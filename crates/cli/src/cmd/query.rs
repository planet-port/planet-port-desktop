// 查询类命令,内容见 Task 5
use crate::client::{CliError, Client};

pub fn info(_client: &Client, _json: bool) -> Result<(), CliError> {
    todo!("Task 5")
}
pub fn listnetworks(
    _client: &Client,
    _planet: &Option<String>,
    _json: bool,
) -> Result<(), CliError> {
    todo!("Task 5")
}
pub fn listpeers(_client: &Client, _planet: &Option<String>, _json: bool) -> Result<(), CliError> {
    todo!("Task 5")
}
pub fn listmoons(_client: &Client, _planet: &Option<String>, _json: bool) -> Result<(), CliError> {
    todo!("Task 5")
}
