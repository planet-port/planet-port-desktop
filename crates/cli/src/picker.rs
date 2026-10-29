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
use std::io::Write;

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
        InquireError::OperationCanceled => eprintln!("已取消(Esc)"),
        InquireError::OperationInterrupted => eprintln!("已取消(Ctrl+C)"),
        other => eprintln!("选择器错误: {other}"),
    }
}

/// 单选(join 等变更类命令):Ok = 选中的 canonical id;Err = 取消/出错。
/// Select 回车即选高亮项,天然满足「高亮 = 选中」,无需定制
pub fn pick_single(planets: &[PlanetOption], verb: &str) -> Result<String, ()> {
    let labels: Vec<String> = planets.iter().map(|p| p.label.clone()).collect();
    let idx = Select::new(&format!("选择 planet:{verb}"), labels)
        .with_help_message("↑↓ 移动,回车确认,Esc/Ctrl+C 取消")
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
        let _ = execute!(std::io::stderr(), Show);
        let _ = terminal::disable_raw_mode();
    }
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
    let mut labels: Vec<&str> = vec!["<全部>"];
    labels.extend(planets.iter().map(|p| p.label.as_str()));
    let hint = "空格勾选,回车确认(空选 = 高亮项;首项 <全部> = 全部),Esc/Ctrl+C 取消";
    let prompt = format!("选择 planet:{verb}");
    let mut cursor = 0usize;
    let mut checked = vec![false; labels.len()];

    if let Err(e) = terminal::enable_raw_mode() {
        eprintln!("终端 raw 模式失败: {e}");
        return Err(());
    }
    let _guard = RawGuard;
    let _ = execute!(std::io::stderr(), Hide);
    let mut lines = render(&prompt, hint, &labels, &checked, cursor).map_err(|e| {
        eprintln!("选择器渲染失败: {e}");
    })?;

    let chosen = loop {
        let ev = match event::read() {
            Ok(ev) => ev,
            Err(e) => {
                eprintln!("\r\n选择器事件读取失败: {e}");
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
                    lines = render(&prompt, hint, &labels, &checked, cursor)
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
        lines = render(&prompt, hint, &labels, &checked, cursor).unwrap_or(lines);
    };
    // 收尾换行,把光标挪出列表区;守卫 Drop 时恢复光标显示与终端模式
    eprintln!();
    match chosen {
        Some(p) => Ok(p),
        None => Err(()), // 取消原因(原始按键)在此处已知,无额外提示必要
    }
}
