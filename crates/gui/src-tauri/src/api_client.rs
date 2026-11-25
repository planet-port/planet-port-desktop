use planet_port_api::{port_file, token_file, DEFAULT_API_PORT};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

/// 常规请求超时（与 CLI 一致；本 GUI 无 identity import 等长耗时端点）
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// 探活超时：仅判断在线与否
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// 统一错误结构，serde 序列化回 webview（前端 GuiError TS 类型与此对齐）。
/// kind 判别值与变体名统一为小写（offline/api/usage），与 TS 契约及 spec 一致。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
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

/// 已装 daemon 二进制路径。路径由 CLI service install 决定（cli/cmd/service/
/// 各平台文件的 BIN_PATH），两侧必须同源改动，GUI 侧无 cli 依赖故本地对齐
#[cfg(target_os = "macos")]
const DAEMON_BIN: &str = "/Library/Application Support/planet-portd/bin/planet-portd";
#[cfg(target_os = "linux")]
const DAEMON_BIN: &str = "/usr/local/bin/planet-portd";
#[cfg(target_os = "windows")]
const DAEMON_BIN: &str = r"C:\Program Files\planet-port\planet-portd.exe";

/// 在线场景读 daemon 版本：/version 响应的 version 字段（= daemon bin 的
/// CARGO_PKG_VERSION，与 CLI --version 同源）。任何失败（离线/token 缺失/
/// 解析失败）回 None，由调用方走离线通道
pub fn fetch_daemon_version() -> Option<String> {
    discover()
        .ok()?
        .get_json(planet_port_api::endpoints::version())
        .ok()?
        .get("version")
        .and_then(|v| v.as_str())
        .map(String::from)
        .filter(|s| !s.is_empty())
}

/// 离线场景读已装 daemon 版本：执行 daemon bin `--version`（clap 输出
/// `planet-portd X.Y.Z`，取第二个空格 token）。bin 缺失/执行失败回 None
pub fn installed_daemon_version() -> Option<String> {
    let out = std::process::Command::new(DAEMON_BIN)
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let ver = text.split_whitespace().nth(1)?;
    (!ver.is_empty()).then(|| ver.to_string())
}

/// port 文件内容 → 端口号；含空白则 trim，非法/0（守护进程未写入完成）一律回退默认端口
fn resolve_port(raw: &str) -> u16 {
    raw.trim().parse::<u16>().ok().filter(|p| *p != 0).unwrap_or(DEFAULT_API_PORT)
}

fn resolve_base() -> String {
    format!("http://127.0.0.1:{}", api_port())
}

/// 当前 daemon API 端口:port 文件 → 合法端口,否则默认 9998。
/// 独立成 pub(crate) 供 bootstrap 轮询复用(daemon 启动可能换端口,须每次重读)
pub(crate) fn api_port() -> u16 {
    std::fs::read_to_string(port_file()).map(|s| resolve_port(&s)).unwrap_or(DEFAULT_API_PORT)
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
    pub fn patch_json(&self, path: &str, body: &Value) -> Result<Value, GuiError> {
        let (c, b) = self.send("PATCH", path, Some(body))?;
        self.parse(c, b)
    }
    pub fn put_json(&self, path: &str, body: &Value) -> Result<Value, GuiError> {
        let (c, b) = self.send("PUT", path, Some(body))?;
        self.parse(c, b)
    }
    pub fn delete(&self, path: &str) -> Result<Value, GuiError> {
        let (c, b) = self.send("DELETE", path, None)?;
        self.parse(c, b)
    }
    /// 二进制端点(如 planet export):2xx 返回原始字节,错误体仍走 JSON 解析。
    /// 不复用 parse 的 Ok 分支——octet-stream 不是 JSON。
    pub fn get_bytes(&self, path: &str) -> Result<Vec<u8>, GuiError> {
        let (status, buf) = self.send("GET", path, None)?;
        if (200..300).contains(&status) {
            Ok(buf)
        } else {
            // 非 2xx 时 parse 恒走 Err 分支;Ok 臂仅为穷尽匹配,不可达
            match self.parse(status, buf) {
                Err(e) => Err(e),
                Ok(_) => Err(GuiError::Api {
                    status,
                    code: None,
                    message: String::from("unexpected success parsing error body"),
                    candidates: None,
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--version` 输出解析:`planet-portd X.Y.Z[-beta.N][-dev] (…)` 取第二 token
    #[test]
    fn version_token_extraction_from_cli_output() {
        let line = "planet-portd 0.5.0-beta.2 (zerotier-core 1.16.2, built 2026-11-25)";
        assert_eq!(line.split_whitespace().nth(1), Some("0.5.0-beta.2"));
        let dev = "planet-portd 0.4.1-dev";
        assert_eq!(dev.split_whitespace().nth(1), Some("0.4.1-dev"));
        let single = "planet-portd";
        assert_eq!(single.split_whitespace().nth(1), None);
    }

    /// Rust↔TS 契约：GuiError 各变体序列化形态必须与 src/api/types.ts 的 GuiError 对齐
    /// （serde tag="kind" → 判别字段 kind，其余字段小写单词、无需 camelCase rename）
    #[test]
    fn gui_error_offline_serializes_as_tagged_object() {
        let v = serde_json::to_value(GuiError::Offline {
            message: String::from("daemon unreachable at http://127.0.0.1:9998"),
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "kind": "offline", "message": "daemon unreachable at http://127.0.0.1:9998" })
        );
    }

    #[test]
    fn gui_error_usage_serializes_as_tagged_object() {
        let v = serde_json::to_value(GuiError::Usage { message: String::from("token file not found") }).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "usage", "message": "token file not found" }));
    }

    #[test]
    fn gui_error_api_serializes_full_fields() {
        let v = serde_json::to_value(GuiError::Api {
            status: 400,
            code: Some(String::from("network_not_found")),
            message: String::from("unknown network"),
            candidates: Some(vec![String::from("9536600adcf2b350")]),
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "kind": "api",
                "status": 400,
                "code": "network_not_found",
                "message": "unknown network",
                "candidates": ["9536600adcf2b350"],
            })
        );
    }

    /// daemon 错误体缺 error/message/candidates 时 code/candidates 为 null（TS 侧 code 可为 null）
    #[test]
    fn gui_error_api_serializes_null_optional_fields() {
        let v = serde_json::to_value(GuiError::Api {
            status: 401,
            code: None,
            message: String::from("invalid token"),
            candidates: None,
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "kind": "api",
                "status": 401,
                "code": null,
                "message": "invalid token",
                "candidates": null,
            })
        );
    }

    /// 端口发现回退：合法值原样采用，空白 trim，非法/0 回退 DEFAULT_API_PORT
    #[test]
    fn resolve_port_accepts_trimmed_valid_port() {
        assert_eq!(resolve_port("8123"), 8123);
        assert_eq!(resolve_port("  8123\n"), 8123);
    }

    #[test]
    fn resolve_port_falls_back_on_invalid_or_zero() {
        assert_eq!(resolve_port(""), DEFAULT_API_PORT);
        assert_eq!(resolve_port("   \n"), DEFAULT_API_PORT);
        assert_eq!(resolve_port("not-a-number"), DEFAULT_API_PORT);
        assert_eq!(resolve_port("0"), DEFAULT_API_PORT);
        // 超出 u16 范围同样视作非法
        assert_eq!(resolve_port("70000"), DEFAULT_API_PORT);
    }

    #[test]
    fn base_url_format_is_loopback_with_port() {
        assert_eq!(format!("http://127.0.0.1:{}", resolve_port("8123")), "http://127.0.0.1:8123");
        assert_eq!(
            format!("http://127.0.0.1:{}", resolve_port("bogus")),
            format!("http://127.0.0.1:{DEFAULT_API_PORT}")
        );
    }

    /// patch_json 负路径:daemon 不可达返回 Offline,不 panic
    /// (正路径由 GUI↔daemon 集成覆盖,此处锁错误契约)
    #[test]
    fn patch_json_offline_daemon_yields_offline_error() {
        let c = ApiClient {
            agent: make_agent(Some(PROBE_TIMEOUT)),
            base: String::from("http://127.0.0.1:1"),
            token: String::from("t"),
        };
        let err = c.patch_json("/planets/x", &serde_json::json!({ "name": "n" })).unwrap_err();
        assert!(matches!(err, GuiError::Offline { .. }), "unreachable 应为 Offline: {err:?}");
    }

    /// put_json 负路径:daemon 不可达返回 Offline,不 panic
    /// (正路径由 GUI↔daemon 集成覆盖,此处锁错误契约;身份模式切换等 PUT 端点)
    #[test]
    fn put_json_offline_daemon_yields_offline_error() {
        let c = ApiClient {
            agent: make_agent(Some(PROBE_TIMEOUT)),
            base: String::from("http://127.0.0.1:1"),
            token: String::from("t"),
        };
        let err = c.put_json("/identity", &serde_json::json!({ "mode": "shared" })).unwrap_err();
        assert!(matches!(err, GuiError::Offline { .. }), "unreachable 应为 Offline: {err:?}");
    }

    /// get_bytes 负路径:daemon 不可达时与 get_json 一致返回 Offline,
    /// 不 panic、不走 JSON parse(export 端点是 octet-stream)。
    /// 正路径(2xx 原样字节)由 GUI↔daemon 集成覆盖,此处锁错误契约。
    #[test]
    fn get_bytes_offline_daemon_yields_offline_error() {
        let c = ApiClient {
            agent: make_agent(Some(PROBE_TIMEOUT)),
            base: String::from("http://127.0.0.1:1"),
            token: String::from("t"),
        };
        let err = c.get_bytes("/planets/x/export").unwrap_err();
        assert!(matches!(err, GuiError::Offline { .. }), "unreachable 应为 Offline: {err:?}");
    }
}
