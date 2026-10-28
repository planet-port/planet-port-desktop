// 变更类命令与 moon 子命令,内容见 Task 6
use crate::client::{CliError, Client};

#[derive(Debug, clap::Subcommand)]
pub enum MoonCmd {
    /// 成员定义见 Task 6,先以隐藏占位保证骨架可编译
    #[command(hide = true)]
    Placeholder,
}

pub fn join(
    _client: &Client,
    _nwid: &str,
    _planet: &Option<String>,
    _json: bool,
) -> Result<(), CliError> {
    todo!("Task 6")
}
pub fn leave(
    _client: &Client,
    _nwid: &str,
    _planet: &Option<String>,
    _json: bool,
) -> Result<(), CliError> {
    todo!("Task 6")
}
pub fn set(
    _client: &Client,
    _nwid: &str,
    _values: &[String],
    _planet: &Option<String>,
    _json: bool,
) -> Result<(), CliError> {
    todo!("Task 6")
}
pub fn orbit(
    _client: &Client,
    _moon_id: &str,
    _seed: &str,
    _planet: &Option<String>,
    _json: bool,
) -> Result<(), CliError> {
    todo!("Task 6")
}
pub fn deorbit(
    _client: &Client,
    _moon_id: &str,
    _planet: &Option<String>,
    _json: bool,
) -> Result<(), CliError> {
    todo!("Task 6")
}
pub fn dump(_client: &Client, _planet: &Option<String>, _json: bool) -> Result<(), CliError> {
    todo!("Task 6")
}
pub fn run_moon(
    _client: &Client,
    _cmd: &MoonCmd,
    _planet: &Option<String>,
    _json: bool,
) -> Result<(), CliError> {
    todo!("Task 6")
}
