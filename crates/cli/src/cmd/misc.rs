// use 与 events 命令,内容见 Task 8
use crate::client::{CliError, Client};

pub fn use_planet(_client: &Client, _planet: &str) -> Result<(), CliError> {
    todo!("Task 8")
}
pub fn events(
    _client: &Client,
    _planet: &Option<String>,
    _limit: usize,
    _follow: bool,
    _json: bool,
) -> Result<(), CliError> {
    todo!("Task 8")
}
