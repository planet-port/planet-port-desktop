//! 顶层 `planet-port install`:一键自安装(离线安装主入口)。
//!
//! 把 CLI 同目录的二进制套装(planet-port + planet-portd)装到系统预设路径,
//! 建 PATH 入口,并装载系统服务(macOS: launchd;Linux: systemd)。具体落点
//! 与软链层随平台不同,收敛在 `cmd::service` 平台模块(macos.rs/linux.rs)。
//!
//! 与 install.sh 的分工:install.sh 只负责在线场景的「下载 + 解压」,落盘后
//! 同样走本命令;解压后的发布包里直接 `sudo ./planet-port install` 即完成
//! 全套离线安装,无脚本依赖。需 root。
//!
//! 升级兜底:安装前记录 running 的 planet——旧版 daemon 被 SIGTERM 会落
//! stopped(新版才保留期望状态),装完后逐个拉回;对新版幂等,已恢复的
//! start 直接成功。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::client::{CliError, Client};
use planet_port_api::endpoints;

use super::service;

pub fn run(yes: bool, json: bool) {
    service::require_root();
    // 0. 版本差异确认:与已装版本不同(升级/降级)时先问一声,同版本幂等
    //    重装不提示;-j 下确认会污染 stdout,必须显式 --yes(同 remove)
    if let Some(old) = installed_version() {
        let new = env!("CARGO_PKG_VERSION");
        if old != new {
            if json && !yes {
                eprintln!("error: --json requires --yes for install when version differs ({old} -> {new})");
                std::process::exit(2);
            }
            if !yes {
                eprint!(
                    "{}",
                    rust_i18n::t!("cli.install.versionConfirm", old = old, new = new)
                );
                std::io::Write::flush(&mut std::io::stderr()).ok();
                let mut answer = String::new();
                if std::io::stdin().read_line(&mut answer).is_err()
                    || !answer.trim().eq_ignore_ascii_case("y")
                {
                    eprintln!("{}", rust_i18n::t!("cli.install.aborted"));
                    std::process::exit(130);
                }
            }
        }
    }

    // 1. 升级兜底:记录当前 running 的 planet(服务停止前取;daemon 不在线
    //    则为空,全新安装本就没有可恢复的)
    let running = running_planets();

    // 2. 定位同目录二进制套装
    let exe = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!("{}", rust_i18n::t!("cli.service.exeNotFound", e = e));
        std::process::exit(1);
    });
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let cli_src = if exe.file_name().and_then(|n| n.to_str()) == Some("planet-port") {
        exe.clone()
    } else {
        dir.join("planet-port")
    };
    let daemon_src = sibling_daemon(&exe);
    for (name, p) in [("planet-port", &cli_src), ("planet-portd", &daemon_src)] {
        if !p.is_file() {
            eprintln!(
                "{}",
                rust_i18n::t!("cli.install.binaryMissing", name = name, dir = dir.display())
            );
            std::process::exit(1);
        }
    }
    // 从已安装路径运行 install:跳过自拷贝(rename 同路径会自删)
    let installed = fs::canonicalize(&cli_src).ok()
        == fs::canonicalize(service::CLI_BIN).ok();

    // 3. 落盘 + PATH 入口(macOS:App Support+软链;Linux:直接 /usr/local/bin)
    service::install_cli(&cli_src, &daemon_src, installed, json);

    // 4. 服务安装(拷 daemon 已在上方完成,函数内的同路径自拷会跳过;
    //    写 plist + bootstrap,RunAtLoad 随装载拉起)
    service::install(json);

    // 5. 升级兜底:等新 daemon 在线后拉回此前 running 的 planet
    if !running.is_empty() && service::wait_online(std::time::Duration::from_secs(15)) {
        let client = Client::new_lenient(None);
        for id in running {
            match client.post_empty(&endpoints::planet_action(&id, "start")) {
                Ok(_) => service::progress(
                    json,
                    rust_i18n::t!("cli.install.planetRestored", id = id).to_string(),
                ),
                Err(e) => eprintln!(
                    "{}",
                    rust_i18n::t!("cli.install.planetRestoreFailed", id = id, e = err_str(&e))
                ),
            }
        }
    }
}

/// 已装 CLI 的版本号(执行 `--version` 取末段);未安装/执行失败返回 None
/// (失败视作版本未知,按"有旧版"处理走确认)
fn installed_version() -> Option<String> {
    if !Path::new(service::CLI_BIN).is_file() {
        return None;
    }
    let out = Command::new(service::CLI_BIN)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let s = String::from_utf8_lossy(&out.stdout);
    s.split_whitespace().last().map(String::from)
}

/// CLI 同目录的 daemon 二进制;从 daemon 自身(planet-portd)运行时用自身
fn sibling_daemon(exe: &Path) -> PathBuf {
    if exe.file_name().and_then(|n| n.to_str()) == Some("planet-portd") {
        return exe.to_path_buf();
    }
    exe.parent().map(|p| p.to_path_buf()).unwrap_or_default().join("planet-portd")
}

/// 安装前 running 的 planet id 清单;daemon 不在线/查询失败返回空
/// (升级兜底是尽力而为,不该因此阻断安装)
fn running_planets() -> Vec<String> {
    let client = Client::new_lenient(std::env::var("PLANET_PORT_URL").ok());
    let v = match client.get_json(endpoints::status()) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    v.get("planets")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|p| p.get("status").and_then(|s| s.as_str()) == Some("running"))
                .filter_map(|p| p.get("id").and_then(|i| i.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn err_str(e: &CliError) -> String {
    match e {
        CliError::Api(code, ae) => format!("error {code}: {}", ae.message),
        CliError::Usage(m) => m.clone(),
        CliError::Io(m) => m.clone(),
        CliError::Offline => "daemon offline".into(),
    }
}
