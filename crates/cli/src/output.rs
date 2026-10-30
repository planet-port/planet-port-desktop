use serde_json::Value;
use std::fmt;
use unicode_width::UnicodeWidthStr;

/// 传统 CLI 风格表格:大写表头 + 空格对齐列、无框线(类 zerotier-cli)。
/// 列宽按显示宽度计算(CJK 全角占 2 列),行尾去填充空格
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

pub fn new_table(headers: &[&str]) -> Table {
    Table {
        headers: headers.iter().map(|h| h.to_string()).collect(),
        rows: Vec::new(),
    }
}

impl Table {
    pub fn add_row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }
}

impl fmt::Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut widths: Vec<usize> =
            self.headers.iter().map(|h| h.width()).collect();
        for row in &self.rows {
            for (i, c) in row.iter().enumerate() {
                if let Some(w) = widths.get_mut(i) {
                    *w = (*w).max(c.width());
                }
            }
        }
        // 最后一列右填充裁掉,避免行尾拖空白
        let line = |cells: &[String]| -> String {
            cells
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let w = widths.get(i).copied().unwrap_or(0);
                    let pad = w.saturating_sub(c.width());
                    if i + 1 == cells.len() {
                        c.clone()
                    } else {
                        format!("{c}{}", " ".repeat(pad + 2))
                    }
                })
                .collect::<String>()
        };
        writeln!(f, "{}", line(&self.headers).trim_end())?;
        for row in &self.rows {
            writeln!(f, "{}", line(row).trim_end())?;
        }
        Ok(())
    }
}

pub fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap());
}

/// 毫秒 epoch → 本地时间(文本模式展示;-j 模式保留原始数值)
pub fn fmt_local_ms(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string()
        })
        .unwrap_or_else(|| ms.to_string())
}

/// 秒 epoch → 本地时间(share 的 expires_at)
pub fn fmt_local_secs(s: u64) -> String {
    chrono::DateTime::from_timestamp(s as i64, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| s.to_string())
}

/// 单值输出(get 命令):字符串裸输出,布尔/数字原样,数组/对象紧凑 JSON
pub fn fmt_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(v).unwrap(),
        other => other.to_string(),
    }
}
