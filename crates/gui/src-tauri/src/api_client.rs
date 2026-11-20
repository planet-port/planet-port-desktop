use planet_port_api::{port_file, token_file, DEFAULT_API_PORT};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

/// 常规请求超时（与 CLI 一致；本 GUI 无 identity import 等长耗时端点）
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// 探活超时：仅判断在线与否
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// 统一错误结构，serde 序列化回 webview（前端 GuiError TS 类型与此对齐）。
/// 字段均为小写单词，无需 camelCase rename。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum GuiError {
    Offline {
        message: String,
    },
    Api {
        status: u16,
        code: Option<String>,
        message: String,
        candidates: Option<Vec<String>>,
    },
    Usage {
        message: String,
    },
}

pub struct ApiClient {
    agent: ureq::Agent,
    base: String,
    token: String,
}

/// 每次调用重新解析端口与 token（daemon 重启可能换端口，与 CLI 行为一致）
pub fn discover() -> Result<ApiClient, GuiError> {
    let token = std::env::var("PLANET_PORT_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
        .or_else(|| {
            std::fs::read_to_string(token_file())
                .ok()
                .map(|s| s.trim().to_string())
        })
        .ok_or_else(|| GuiError::Usage {
            message: format!("token file not found: {}", token_file().display()),
        })?;
    Ok(ApiClient {
        agent: make_agent(Some(REQUEST_TIMEOUT)),
        base: resolve_base(),
        token,
    })
}

/// token 文件存在 = 安装过（CLI 同款判定来源）
pub fn installed() -> bool {
    token_file().exists()
}

fn resolve_base() -> String {
    let mut port = DEFAULT_API_PORT;
    if let Ok(s) = std::fs::read_to_string(port_file()) {
        if let Ok(p) = s.trim().parse::<u16>() {
            if p != 0 {
                port = p;
            }
        }
    }
    format!("http://127.0.0.1:{port}")
}

fn make_agent(timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .http_status_as_error(false)
            .timeout_global(timeout)
            .build(),
    )
}

/// 探活：2s 超时 GET /version；任何 HTTP 响应（含 401）= 端口上有服务
pub fn probe_online() -> bool {
    let agent = make_agent(Some(PROBE_TIMEOUT));
    let req = ureq::http::Request::builder()
        .method("GET")
        .uri(format!("{}/version", resolve_base()))
        .body(Vec::new());
    match req {
        Ok(r) => agent.run(r).is_ok(),
        Err(_) => false,
    }
}

impl ApiClient {
    /// ureq 3 惯用法照抄 CLI client.rs：http::Request + agent.run + read_to_vec
    fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Vec<u8>), GuiError> {
        let url = format!("{}{}", self.base, path);
        let mut builder = ureq::http::Request::builder()
            .method(method)
            .uri(&url)
            .header("X-ZT1-Auth", &self.token);
        let bytes = match body {
            Some(b) => {
                builder = builder.header("Content-Type", "application/json");
                serde_json::to_vec(b).map_err(|e| GuiError::Usage {
                    message: e.to_string(),
                })?
            }
            None => Vec::new(),
        };
        let req = builder.body(bytes).map_err(|e| GuiError::Usage {
            message: e.to_string(),
        })?;
        let mut resp = self.agent.run(req).map_err(|_| GuiError::Offline {
            message: format!("daemon unreachable at {url}"),
        })?;
        let status = resp.status().as_u16();
        let buf = resp
            .body_mut()
            .read_to_vec()
            .map_err(|e| GuiError::Offline {
                message: e.to_string(),
            })?;
        Ok((status, buf))
    }

    fn parse(&self, status: u16, buf: Vec<u8>) -> Result<Value, GuiError> {
        let text = String::from_utf8_lossy(&buf).into_owned();
        if (200..300).contains(&status) {
            Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
        } else {
            // daemon 错误体：{"error": code, "message": ..., "candidates": [...](可选)}
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            Err(GuiError::Api {
                status,
                code: v.get("error").and_then(|x| x.as_str()).map(String::from),
                message: v
                    .get("message")
                    .and_then(|x| x.as_str())
                    .unwrap_or(&text)
                    .to_string(),
                candidates: v.get("candidates").and_then(|x| x.as_array()).map(|a| {
                    a.iter()
                        .filter_map(|i| i.as_str().map(String::from))
                        .collect()
                }),
            })
        }
    }

    pub fn get_json(&self, path: &str) -> Result<Value, GuiError> {
        let (c, b) = self.send("GET", path, None)?;
        self.parse(c, b)
    }
    pub fn post_empty(&self, path: &str) -> Result<Value, GuiError> {
        let (c, b) = self.send("POST", path, None)?;
        self.parse(c, b)
    }
    pub fn post_json(&self, path: &str, body: &Value) -> Result<Value, GuiError> {
        let (c, b) = self.send("POST", path, Some(body))?;
        self.parse(c, b)
    }
    pub fn delete(&self, path: &str) -> Result<Value, GuiError> {
        let (c, b) = self.send("DELETE", path, None)?;
        self.parse(c, b)
    }
}
