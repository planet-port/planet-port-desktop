//! 网络分享:终端二维码 + 网络 ID 复制剪贴板。二维码规范对齐 iOS 版
//! NetworkQRSheet——内容为网络 ID 16 位 hex 纯文本(与 App 内扫码填 ID
//! 互逆,非 URL)、纠错级别 M;展示三件套 = 二维码 + 网络名 + 网络 ID

use crate::client::{CliError, Client};
use crate::cmd::resolve_registered;
use crate::output::print_json;
use qrcode::{EcLevel, QrCode};

pub fn share(client: &Client, nwid: &str, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    let pid = resolve_registered(client, planet)?;
    let n = crate::cmd::mutate::find_network(client, nwid, &pid)?;
    if json {
        // -j:stdout 仅 NetworkEntry(二维码/剪贴板属展示层,人话走 stderr)
        let v = serde_json::to_value(&n).map_err(|e| CliError::Io(e.to_string()))?;
        print_json(&v);
        return Ok(());
    }
    let title = n.name.clone().unwrap_or_else(|| n.nwid.clone());
    let code = QrCode::with_error_correction_level(n.nwid.as_bytes(), EcLevel::M)
        .map_err(|e| CliError::Io(format!("二维码生成失败: {e}")))?;
    println!("{title}\n");
    println!("{}", render_qr(&code));
    println!("{}", n.nwid);
    // 剪贴板:各平台 CLI 均无需授权;无显示服务器等不支持环境降级提示,
    // 不报错中断(分享主体是二维码与 ID 文本)
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(n.nwid.clone())) {
        Ok(()) => eprintln!("{}", rust_i18n::t!("cli.share.copied")),
        Err(_) => eprintln!("{}", rust_i18n::t!("cli.share.copyFailed")),
    }
    Ok(())
}

/// 半块字符渲染:每字符承载两行模块(▀ 上半 ▄ 下半 █ 全黑 ' ' 全白),
/// 四周留足 QR 规范 4 模块 quiet zone:左右各 4 空格(1 字符 = 1 模块),
/// 上下各 2 空行(1 行文本 = 2 模块);dark 模块渲染为前景色字符,
/// 依赖终端前景/背景对比度,主流黑底白字终端即为"白块数据点"
fn render_qr(code: &QrCode) -> String {
    let w = code.width();
    // 水平 4 空格 = 4 模块
    let quiet = 4;
    let blank = " ".repeat(quiet);
    // 垂直上下各 2 空行 = 各 4 模块
    let mut out = String::from("\n\n");
    for y in (0..w).step_by(2) {
        let mut line = String::with_capacity(w + quiet * 2);
        line.push_str(&blank);
        for x in 0..w {
            let top = matches!(code[(x, y)], qrcode::Color::Dark);
            let bottom = y + 1 < w && matches!(code[(x, y + 1)], qrcode::Color::Dark);
            line.push(match (top, bottom) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        line.push_str(&blank);
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("\n\n");
    out
}
