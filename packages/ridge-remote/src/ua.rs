//! User-Agent 驱动的 UI 分叉：决定给浏览器发完整桌面 SPA 还是轻量移动 SPA。
//!
//! 这是 **单一事实来源（SSOT）**：局域网远控服务端（桌面 Tauri app
//! `src-tauri/src/remote/server.rs`）与公网远控中继（ridge-cloud 的
//! `spa_fallback`）都应复用这里的判定，避免"手机/桌面"分叉规则在两个入口漂移。
//! 两端 serve 的本就是同一套客户端产物（wind `web-remote-dist` 桌面 SPA /
//! `static/remote` 移动 SPA），分叉决策也理应共用一份。

/// 标记移动/触屏浏览器的 User-Agent 子串。UA 命中任一即发轻量移动 SPA，
/// 其余一律发完整桌面 SPA。
pub const MOBILE_UA_MARKERS: [&str; 6] = [
    "android",
    "iphone",
    "ipad",
    "ipod",
    "mobile",
    "windows phone",
];

/// 原始 User-Agent 是否为移动/触屏浏览器（对 `MOBILE_UA_MARKERS` 大小写无关子串匹配）。
pub fn is_mobile_ua(ua: &str) -> bool {
    let ua = ua.to_ascii_lowercase();
    MOBILE_UA_MARKERS.iter().any(|m| ua.contains(m))
}

/// 是否优先发桌面 SPA：先尊重显式覆盖（`?ui=desktop` / `?ui=mobile`，供测试与
/// 边缘浏览器），否则回退到 UA 嗅探。返回 `true` 表示桌面 SPA，`false` 表示移动 SPA。
///
/// 注意：本函数只做"想要哪套 UI"的判定；调用方仍需校验对应产物目录是否存在
/// （桌面产物缺失时应自行回退到移动 SPA）。
pub fn prefer_desktop_ui(ua: &str, ui_override: Option<&str>) -> bool {
    match ui_override {
        Some("desktop") => true,
        Some("mobile") => false,
        _ => !is_mobile_ua(ua),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_uas_detected() {
        for ua in [
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Safari",
            "Mozilla/5.0 (Linux; Android 14; Pixel 8) Chrome Mobile",
            "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X)",
        ] {
            assert!(is_mobile_ua(ua), "should detect mobile: {ua}");
        }
    }

    #[test]
    fn desktop_uas_not_mobile() {
        for ua in [
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/120 Safari",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) Safari",
            "",
        ] {
            assert!(!is_mobile_ua(ua), "should NOT be mobile: {ua}");
        }
    }

    #[test]
    fn override_wins_over_ua() {
        let iphone = "Mozilla/5.0 (iPhone)";
        let windows = "Mozilla/5.0 (Windows NT 10.0)";
        // 显式覆盖优先于 UA 嗅探
        assert!(prefer_desktop_ui(iphone, Some("desktop")));
        assert!(!prefer_desktop_ui(windows, Some("mobile")));
        // 无覆盖 → 跟随 UA
        assert!(!prefer_desktop_ui(iphone, None));
        assert!(prefer_desktop_ui(windows, None));
    }
}
