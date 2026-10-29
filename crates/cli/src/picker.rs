//! 交互式 planet 选择:多 planet 且未显式指定(--planet / PLANET_PORT_PLANET)时
//! 弹出列表。空格勾选、回车确认、Esc/Ctrl+C 取消。
//! 仅在 stdin 为 TTY 且非 --json 时启用(脚本/管道保持原聚合/409 行为)。

use inquire::{InquireError, MultiSelect, Select};

/// 选择器候选(id 用于回填 --planet,label 用于展示)
pub struct PlanetOption {
    pub id: String,
    pub label: String,
}

/// 多选结果:All = 全部(勾选「<全部>」或空选);Subset = 选中的 canonical id 集
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

/// 单选(join 等变更类命令):Ok = 选中的 canonical id;Err = 取消/出错
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

/// 多选(info 等可批量命令):首项「<全部>」;勾选它或直接回车(空选)= 全部
pub fn pick_multi(planets: &[PlanetOption], verb: &str) -> Result<MultiPick, ()> {
    let mut labels: Vec<String> = vec!["<全部>".into()];
    labels.extend(planets.iter().map(|p| p.label.clone()));
    let sel = MultiSelect::new(&format!("选择 planet:{verb}"), labels)
        .with_help_message("空格勾选,回车确认(空选 = 全部),Esc/Ctrl+C 取消")
        .raw_prompt();
    match sel {
        Ok(chosen) => {
            // 勾中「全部」(下标 0)→ 全量;候选 planet 从下标 1 起
            if chosen.is_empty() || chosen.iter().any(|lo| lo.index == 0) {
                return Ok(MultiPick::All);
            }
            Ok(MultiPick::Subset(
                chosen
                    .into_iter()
                    .filter_map(|lo| planets.get(lo.index - 1).map(|p| p.id.clone()))
                    .collect(),
            ))
        }
        Err(e) => {
            cancelled(&e);
            Err(())
        }
    }
}
