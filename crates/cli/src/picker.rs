//! 交互式 planet 选择:多 planet 且未显式指定(--planet / PLANET_PORT_PLANET)时
//! 弹出列表。空格勾选、回车确认、Esc/Ctrl+C 取消。
//! 仅在 stdin 为 TTY 且非 --json 时启用(脚本/管道保持原聚合/409 行为)。
//!
//! 多选列表为 crossterm 自绘(inquire 的 MultiSelect 不暴露回车时刻的高亮
//! 下标,无法实现「空选回车 = 高亮项」):高亮即选中——没有任何勾选时,
//! 当前高亮项渲染为 [x] 且回车即选它;有勾选则按勾选集合算。

use crossterm::{
    cursor::{Hide, MoveUp, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, Clear, ClearType},
};
use inquire::{InquireError, Select};
use signal_hook::{
    consts::{SIGHUP, SIGINT, SIGTERM},
    iterator::Signals,
};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 选择器候选(id 用于回填 --planet,label 用于展示)
pub struct PlanetOption {
    pub id: String,
    pub label: String,
}

/// 多选结果:All = 全部(选中「<全部>」);Subset = 选中的 canonical id 集
pub enum MultiPick {
    All,
    Subset(Vec<String>),
}

fn cancelled(e: &InquireError) {
    match e {
        InquireError::OperationCanceled => eprintln!("{}", rust_i18n::t!("cli.picker.cancelEsc")),
        InquireError::OperationInterrupted => {
            eprintln!("{}", rust_i18n::t!("cli.picker.cancelCtrlC"))
        }
        other => eprintln!("{}", rust_i18n::t!("cli.picker.error", e = other)),
    }
}

/// 单选(join 等变更类命令):Ok = 选中的 canonical id;Err = 取消/出错。
/// Select 回车即选高亮项,天然满足「高亮 = 选中」,无需定制
pub fn pick_single(planets: &[PlanetOption], verb: &str) -> Result<String, ()> {
    // inquire 内部同走 crossterm 事件源(死 TTY 同样会卡),信号线程保证
    // SIGHUP/SIGTERM 到达即恢复终端并终止;看门狗因无迭代可埋点,不适用
    spawn_signal_guard();
    let labels: Vec<String> = planets.iter().map(|p| p.label.clone()).collect();
    let idx = Select::new(&rust_i18n::t!("cli.picker.prompt", verb = verb), labels)
        .with_help_message(&rust_i18n::t!("cli.picker.helpSingle"))
        .raw_prompt();
    match idx {
        Ok(lo) => Ok(planets[lo.index].id.clone()),
        Err(e) => {
            cancelled(&e);
            Err(())
        }
    }
}

/// raw 模式守卫:任何退出路径(含 ?)都恢复终端,防止 shell 变成无回显僵尸
struct RawGuard;
impl Drop for RawGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// 恢复终端(显示光标 + 退出 raw 模式);正常收尾与异常终止路径共用
fn restore_terminal() {
    let _ = execute!(std::io::stderr(), Show);
    let _ = terminal::disable_raw_mode();
}

/// 按信号本义终止:恢复终端 → 还原默认处置 → 重发信号,保留
/// killed-by-signal 的退出语义。libc 是既有依赖,不引低层封装
fn die_by_signal(sig: i32) -> ! {
    restore_terminal();
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
    // 默认处置必然终止;万一返回,兜底退出而不是 panic(panic 只杀本线程)
    std::process::exit(130)
}

/// 信号防护线程:SIGHUP/SIGTERM/SIGINT 的处置不依赖主循环轮询。
/// 实测 crossterm 0.29 的 event::poll 在 TTY 失效(PTY 主端关闭,read
/// 持续 EIO)时内部热循环永不返回——主循环走不到信号检查,SIGTERM
/// 杀不掉、CPU 打满。改为独立线程阻塞等信号,收到即恢复终端并按信号
/// 本义终止;PTY 主端消亡时内核投递 SIGHUP,正是选择器该退出的时机。
/// 线程随进程退出回收(一次性 CLI,无需显式停止);注册失败降级为无防护
fn spawn_signal_guard() {
    if let Ok(signals) = Signals::new([SIGTERM, SIGINT, SIGHUP]) {
        std::thread::spawn(move || {
            let mut signals = signals;
            for sig in signals.forever() {
                die_by_signal(sig);
            }
        });
    }
}

/// 心跳看门狗:兜底「事件源卡死且无信号送达」的残余场景(TTY 已 EIO
/// 但 SIGHUP 未及投递)。主循环每迭代 +1(正常节奏 poll 100ms ≥10Hz),
/// 连续 3s 未走动判定卡死,恢复终端后退出码 1。仅 pick_multi 可用——
/// pick_single 走 inquire 内部循环,没有可埋点的迭代
fn spawn_watchdog(heartbeat: Arc<AtomicU64>) {
    std::thread::spawn(move || {
        let mut last = 0u64;
        let mut stalled = 0u32;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let now = heartbeat.load(Ordering::Relaxed);
            if now == last {
                stalled += 1;
                if stalled >= 6 {
                    restore_terminal();
                    std::process::exit(1);
                }
            } else {
                stalled = 0;
                last = now;
            }
        }
    });
}

/// 渲染一屏并返回行数(供下次重绘回退用)。on = 勾选或(空选时的)高亮
fn render(
    prompt: &str,
    hint: &str,
    labels: &[&str],
    checked: &[bool],
    cursor: usize,
) -> std::io::Result<usize> {
    let none_checked = !checked.iter().any(|c| *c);
    let mut out = String::new();
    out.push_str(prompt);
    out.push_str("\r\n");
    for (i, l) in labels.iter().enumerate() {
        let on = checked[i] || (none_checked && i == cursor);
        let mark = if on { "[x]" } else { "[ ]" };
        let ptr = if i == cursor { "❯ " } else { "  " };
        let line = format!("{ptr}{mark} {l}");
        // 光标行加亮(青色加粗),其余行保持终端默认色
        if i == cursor {
            out.push_str(&format!("\x1b[1;36m{line}\x1b[0m\r\n"));
        } else {
            out.push_str(&format!("{line}\r\n"));
        }
    }
    out.push_str(&format!("\x1b[2m{hint}\x1b[0m\r\n"));
    let mut err = std::io::stderr();
    write!(err, "{out}")?;
    err.flush()?;
    // 提示行 + 选项 + 标题;光标此刻在下一行行首,MoveUp 这些行即回到标题
    Ok(2 + labels.len())
}

/// 多选(info 等可批量命令):首项「<全部>」= 全量。空选时高亮即选中
/// (回车选高亮项);有勾选按勾选算
pub fn pick_multi(planets: &[PlanetOption], verb: &str) -> Result<MultiPick, ()> {
    let all = rust_i18n::t!("cli.picker.all").to_string();
    let mut labels: Vec<String> = vec![all];
    labels.extend(planets.iter().map(|p| p.label.clone()));
    let labels: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let hint = rust_i18n::t!("cli.picker.helpMulti").to_string();
    let prompt = rust_i18n::t!("cli.picker.prompt", verb = verb).to_string();
    let mut cursor = 0usize;
    let mut checked = vec![false; labels.len()];

    if let Err(e) = terminal::enable_raw_mode() {
        eprintln!("{}", rust_i18n::t!("cli.picker.rawModeFailed", e = e));
        return Err(());
    }
    let _guard = RawGuard;
    let _ = execute!(std::io::stderr(), Hide);
    let mut lines = render(&prompt, &hint, &labels, &checked, cursor).map_err(|e| {
        eprintln!("{}", rust_i18n::t!("cli.picker.renderFailed", e = e));
    })?;
    // 信号线程 + 心跳看门狗(见各自注释):前者保证死 TTY 场景随 SIGHUP
    // 即刻终止,后者兜底事件源卡死且无信号的残余情况
    spawn_signal_guard();
    let heartbeat = Arc::new(AtomicU64::new(0));
    spawn_watchdog(heartbeat.clone());

    let chosen = loop {
        heartbeat.fetch_add(1, Ordering::Relaxed);
        // 事件轮询而非阻塞 read:空窗期回到信号检查
        let ev = match event::poll(std::time::Duration::from_millis(100)) {
            Ok(true) => match event::read() {
                Ok(ev) => ev,
                Err(e) => {
                    eprint!("\r\n");
                    eprintln!("{}", rust_i18n::t!("cli.picker.eventReadFailed", e = e));
                    break None;
                }
            },
            Ok(false) => continue,
            Err(e) => {
                eprint!("\r\n");
                eprintln!("{}", rust_i18n::t!("cli.picker.eventPollFailed", e = e));
                break None;
            }
        };
        // 只响应按下;其余事件(鼠标/ resize)触发重绘
        let key = match ev {
            Event::Key(k) if k.kind == KeyEventKind::Press => k,
            other => {
                if matches!(other, Event::Resize(_, _)) {
                    if lines > 0 {
                        let _ = execute!(
                            std::io::stderr(),
                            MoveUp(lines as u16),
                            Clear(ClearType::FromCursorDown)
                        );
                    }
                    lines = render(&prompt, &hint, &labels, &checked, cursor)
                        .unwrap_or(lines);
                }
                continue;
            }
        };
        match (key.code, key.modifiers) {
            (KeyCode::Up | KeyCode::Char('k'), _) => {
                cursor = (cursor + labels.len() - 1) % labels.len();
            }
            (KeyCode::Down | KeyCode::Char('j'), _) => {
                cursor = (cursor + 1) % labels.len();
            }
            (KeyCode::Char(' '), _) => checked[cursor] = !checked[cursor],
            (KeyCode::Enter, _) => {
                // 空选:高亮项即选中(渲染时已显示 [x]);否则按勾选集合
                let none_checked = !checked.iter().any(|c| *c);
                if none_checked {
                    checked[cursor] = true;
                }
                if checked[0] {
                    break Some(MultiPick::All);
                }
                let ids = checked
                    .iter()
                    .enumerate()
                    .skip(1)
                    .filter(|&(_, &c)| c)
                    .map(|(i, _)| planets[i - 1].id.clone())
                    .collect();
                break Some(MultiPick::Subset(ids));
            }
            (KeyCode::Esc, _) => break None,
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => break None,
            _ => continue, // 无状态变化的按键不重绘
        }
        if lines > 0 {
            let _ = execute!(
                std::io::stderr(),
                MoveUp(lines as u16),
                Clear(ClearType::FromCursorDown)
            );
        }
        lines = render(&prompt, &hint, &labels, &checked, cursor).unwrap_or(lines);
    };
    // 收尾换行,把光标挪出列表区;守卫 Drop 时恢复光标显示与终端模式
    eprintln!();
    match chosen {
        Some(p) => Ok(p),
        None => Err(()), // 取消原因(原始按键)在此处已知,无额外提示必要
    }
}
