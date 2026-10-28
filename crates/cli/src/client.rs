use planet_port_api::types::ApiError;
use planet_port_api::{token_file, DEFAULT_API_PORT};
use serde_json::Value;
use std::time::Duration;

/// 常规请求全局超时;events --follow 的轮询请求走无超时 agent,长挂不受影响
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub enum CliError {
    Offline,
    Api(u16, ApiError),
    Usage(String),
    Io(String),
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Offline => 3,
            // planet 歧义(409 + 候选)与用法错误同为退出码 2
            CliError::Api(409, e) if e.candidates.is_some() => 2,
            CliError::Api(..) => 1,
            CliError::Usage(_) => 2,
            CliError::Io(_) => 1,
        }
    }
}

pub struct Client {
    agent: ureq::Agent,
    poll_agent: ureq::Agent,
    base: String,
    token: String,
}

impl Client {
    pub fn new(api_url: Option<String>) -> Result<Self, CliError> {
        let base = match api_url {
            Some(u) => u.trim_end_matches('/').to_string(),
            None => format!("http://127.0.0.1:{DEFAULT_API_PORT}"),
        };
        let token = match std::env::var("PLANET_PORT_TOKEN") {
            Ok(t) if !t.is_empty() => t,
            _ => std::fs::read_to_string(token_file())
                .map_err(|_| {
                    CliError::Usage(format!(
                        "token file not found: {} (dev: set PLANET_PORT_TOKEN)",
                        token_file().display()
                    ))
                })?
                .trim()
                .to_string(),
        };
        let agent = Self::make_agent(Some(REQUEST_TIMEOUT));
        let poll_agent = Self::make_agent(None);
        Ok(Self {
            agent,
            poll_agent,
            base,
            token,
        })
    }

    fn make_agent(timeout: Option<Duration>) -> ureq::Agent {
        ureq::Agent::new_with_config(
            ureq::config::Config::builder()
                .http_status_as_error(false)
                .timeout_global(timeout)
                .build(),
        )
    }

    /// 底层请求:返回状态码与原始字节,文本/二进制端点共用同一错误语义
    fn request_bytes(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Vec<u8>), CliError> {
        self.send(&self.agent, method, path, body)
    }

    fn send(
        &self,
        agent: &ureq::Agent,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Vec<u8>), CliError> {
        let url = format!("{}{}", self.base, path);
        // ureq 3.5 无 Agent::request(method, url),改用 http::Request + agent.run()
        let mut builder = ureq::http::Request::builder()
            .method(method)
            .uri(&url)
            .header("X-ZT1-Auth", &self.token);
        let bytes = match body {
            Some(b) => {
                builder = builder.header("Content-Type", "application/json");
                serde_json::to_vec(b).map_err(|e| CliError::Io(e.to_string()))?
            }
            None => Vec::new(),
        };
        let req = builder.body(bytes).map_err(|e| CliError::Io(e.to_string()))?;
        // 连接失败统一视为 daemon 不在线(退出码 3);超时单独提示,避免误报"未运行"
        let mut resp = agent.run(req).map_err(|e| match e {
            ureq::Error::Timeout(_) => CliError::Io(format!(
                "request to {method} {url} timed out after {}s",
                REQUEST_TIMEOUT.as_secs()
            )),
            _ => CliError::Offline,
        })?;
        let status = resp.status().as_u16();
        let buf = resp
            .body_mut()
            .read_to_vec()
            .map_err(|e| CliError::Io(e.to_string()))?;
        Ok((status, buf))
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, String), CliError> {
        let (status, buf) = self.request_bytes(method, path, body)?;
        let text = String::from_utf8_lossy(&buf).into_owned();
        Ok((status, text))
    }

    fn parse(&self, code: u16, text: String) -> Result<Value, CliError> {
        if (200..300).contains(&code) {
            Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
        } else {
            Err(Self::api_error(code, text))
        }
    }

    pub fn get_json(&self, path: &str) -> Result<Value, CliError> {
        let (c, t) = self.request("GET", path, None)?;
        self.parse(c, t)
    }
    /// events --follow 专用的轮询 GET:无全局超时,daemon 偶发阻塞不会中断长挂
    pub fn get_json_polling(&self, path: &str) -> Result<Value, CliError> {
        let (c, buf) = self.send(&self.poll_agent, "GET", path, None)?;
        self.parse(c, String::from_utf8_lossy(&buf).into_owned())
    }
    /// GET 纯文本端点(如 /dump),2xx 返回正文,非 2xx 仍走 ApiError 解析
    pub fn get_text(&self, path: &str) -> Result<String, CliError> {
        let (c, t) = self.request("GET", path, None)?;
        if (200..300).contains(&c) {
            Ok(t)
        } else {
            Err(Self::api_error(c, t))
        }
    }
    fn api_error(code: u16, text: String) -> CliError {
        let api = serde_json::from_str(&text).unwrap_or(ApiError {
            error: format!("http{code}"),
            message: text,
            candidates: None,
        });
        CliError::Api(code, api)
    }
    pub fn post_empty(&self, path: &str) -> Result<Value, CliError> {
        let (c, t) = self.request("POST", path, None)?;
        self.parse(c, t)
    }
    pub fn post_json(&self, path: &str, body: &Value) -> Result<Value, CliError> {
        let (c, t) = self.request("POST", path, Some(body))?;
        self.parse(c, t)
    }
    pub fn patch_json(&self, path: &str, body: &Value) -> Result<Value, CliError> {
        let (c, t) = self.request("PATCH", path, Some(body))?;
        self.parse(c, t)
    }
    pub fn delete(&self, path: &str) -> Result<Value, CliError> {
        let (c, t) = self.request("DELETE", path, None)?;
        self.parse(c, t)
    }
    /// GET 二进制端点(如 planet export);非 2xx 走 ApiError,仅传输错误才算 Offline
    pub fn get_bytes(&self, path: &str) -> Result<Vec<u8>, CliError> {
        let (c, buf) = self.request_bytes("GET", path, None)?;
        if (200..300).contains(&c) {
            Ok(buf)
        } else {
            Err(Self::api_error(c, String::from_utf8_lossy(&buf).into_owned()))
        }
    }
}
