use planet_port_api::types::ApiError;
use planet_port_api::{token_file, DEFAULT_API_PORT};
use serde_json::Value;

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
        let agent = ureq::Agent::new_with_config(
            ureq::config::Config::builder()
                .http_status_as_error(false)
                .build(),
        );
        Ok(Self { agent, base, token })
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, String), CliError> {
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
        // 连接失败统一视为 daemon 不在线(退出码 3)
        let mut resp = self.agent.run(req).map_err(|_| CliError::Offline)?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| CliError::Io(e.to_string()))?;
        Ok((status, text))
    }

    fn parse(&self, code: u16, text: String) -> Result<Value, CliError> {
        if (200..300).contains(&code) {
            Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
        } else {
            let api = serde_json::from_str(&text).unwrap_or(ApiError {
                error: format!("http{code}"),
                message: text,
                candidates: None,
            });
            Err(CliError::Api(code, api))
        }
    }

    pub fn get_json(&self, path: &str) -> Result<Value, CliError> {
        let (c, t) = self.request("GET", path, None)?;
        self.parse(c, t)
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
    pub fn get_bytes(&self, path: &str) -> Result<Vec<u8>, CliError> {
        let url = format!("{}{}", self.base, path);
        let mut resp = self
            .agent
            .get(&url)
            .header("X-ZT1-Auth", &self.token)
            .call()
            .map_err(|_| CliError::Offline)?;
        if !resp.status().is_success() {
            return Err(CliError::Offline);
        }
        let buf = resp
            .body_mut()
            .read_to_vec()
            .map_err(|e| CliError::Io(e.to_string()))?;
        Ok(buf)
    }
}
