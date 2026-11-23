fn main() {
    // 构建时间注入关于页(UTC+8 固定偏移,不引 chrono;civil 日期用
    // Howard Hinnant days→y/m/d 算法)。tauri_build 的 rerun 规则决定
    // build.rs 重跑时机,本地增量编译可能沿用上次时间,CI 全新环境必新鲜
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + 8 * 3600;
    let (days, rem) = (secs / 86400, secs % 86400);
    let (h, mi, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    println!("cargo:rustc-env=PP_BUILD_TIME={y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02} +0800");
    tauri_build::build()
}
