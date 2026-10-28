use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};
use serde_json::Value;

pub fn new_table(headers: &[&str]) -> Table {
    let mut t = Table::new();
    t.load_preset(UTF8_FULL_CONDENSED).set_header(headers);
    t
}

pub fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap());
}
