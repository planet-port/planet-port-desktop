// planet 管理子命令组,内容见 Task 7
use crate::client::{CliError, Client};

#[derive(Debug, clap::Subcommand)]
pub enum PlanetCmd {
    /// 成员定义见 Task 7,先以隐藏占位保证骨架可编译
    #[command(hide = true)]
    Placeholder,
}

pub fn run(
    _client: &Client,
    _cmd: &PlanetCmd,
    _json: bool,
) -> Result<(), CliError> {
    todo!("Task 7")
}
