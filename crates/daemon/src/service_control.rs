//! 官方 ZeroTier 服务的探测与停用(stop + disable 自启,不卸载)。
//! 供身份导入在写入全局身份前调用,消除同身份双开互踢(官方 issue #2433)。
//! 全部为子进程阻塞实现,调用方须放 spawn_blocking(锁与线程纪律红线)。

use std::process::Command;

/// 官方服务处置结果(not_installed = 本机未装官方客户端,导入照常)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfficialServiceAction {
    StoppedAndDisabled,
    NotInstalled,
    Failed(String),
}

/// 跑子进程:成功返回 stdout,失败返回带程序名的错误串
fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("{:?}: {e}", cmd.get_program()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{:?}: {}{}",
            cmd.get_program(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        ))
    }
}

#[cfg(target_os = "macos")]
pub fn stop_and_disable() -> OfficialServiceAction {
    use std::path::Path;

    // 探测:launchd 已加载或 plist 在盘即视为安装
    let installed = Command::new("launchctl")
        .args(["print", "system/com.zerotier.one"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
        || Path::new("/Library/LaunchDaemons/com.zerotier.one.plist").exists();
    if !installed {
        return OfficialServiceAction::NotInstalled;
    }
    // 未加载时 bootout 报错属预期,忽略其错误继续 disable
    let _ = run(Command::new("launchctl").args(["bootout", "system/com.zerotier.one"]));
    match run(Command::new("launchctl").args(["disable", "system/com.zerotier.one"])) {
        Ok(_) => OfficialServiceAction::StoppedAndDisabled,
        Err(e) => OfficialServiceAction::Failed(e),
    }
}

#[cfg(target_os = "linux")]
pub fn stop_and_disable() -> OfficialServiceAction {
    let installed = match run(Command::new("systemctl").args(["list-unit-files", "zerotier-one.service"])) {
        Ok(out) => out.contains("zerotier-one"),
        Err(_) => false,
    };
    if !installed {
        return OfficialServiceAction::NotInstalled;
    }
    // 已停止时 stop 为幂等成功
    if let Err(e) = run(Command::new("systemctl").args(["stop", "zerotier-one"])) {
        return OfficialServiceAction::Failed(e);
    }
    match run(Command::new("systemctl").args(["disable", "zerotier-one"])) {
        Ok(_) => OfficialServiceAction::StoppedAndDisabled,
        Err(e) => OfficialServiceAction::Failed(e),
    }
}

#[cfg(target_os = "windows")]
pub fn stop_and_disable() -> OfficialServiceAction {
    use std::thread::sleep;
    use std::time::Duration;

    /// sc stop 是异步的:命令返回时服务多为 STOP_PENDING,官方进程可能仍
    /// 持同一身份运行(正是身份导入要消除的双开互踢)。因此 stop 后必须
    /// 轮询 sc query 直到出现 STOPPED 才算停干净。
    const POLL_TIMES: usize = 30;
    const POLL_INTERVAL_MS: u64 = 500;

    // query 对已存在服务成功返回状态文本,对不存在的服务失败返回非零
    let query = || -> Result<String, String> {
        run(Command::new("sc.exe").args(["query", "ZeroTierOneService"]))
    };
    let status = match query() {
        Ok(out) => out,
        Err(_) => return OfficialServiceAction::NotInstalled,
    };
    if !status.contains("STOPPED") {
        // 已停止时 stop 报非运行错误,忽略其错误继续轮询确认
        let _ = run(Command::new("sc.exe").args(["stop", "ZeroTierOneService"]));
        let mut stopped = false;
        for _ in 0..POLL_TIMES {
            sleep(Duration::from_millis(POLL_INTERVAL_MS));
            match query() {
                Ok(out) if out.contains("STOPPED") => {
                    stopped = true;
                    break;
                }
                Ok(_) => {}
                Err(e) => return OfficialServiceAction::Failed(e),
            }
        }
        if !stopped {
            return OfficialServiceAction::Failed(
                "service did not stop within 15s; official ZeroTier may still \
                 hold the same identity"
                    .to_string(),
            );
        }
    }
    match run(Command::new("sc.exe").args(["config", "ZeroTierOneService", "start=", "disabled"])) {
        Ok(_) => OfficialServiceAction::StoppedAndDisabled,
        Err(e) => OfficialServiceAction::Failed(e),
    }
}
