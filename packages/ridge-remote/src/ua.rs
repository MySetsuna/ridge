//! Remote UI 分叉决定：默认发轻量移动 SPA，仅 URL 显式 `?ui=desktop` 才发
//! 完整桌面 SPA。UA / 窗口宽度 / 历史偏好一律不算数。
//!
//! 这是 **单一事实来源（SSOT）**：局域网远控服务端（桌面 Tauri app
//! `src-tauri/src/remote/server.rs`）与公网远控中继（ridge-cloud 的
//! `spa_fallback`）都应复用这里的判定，避免"手机/桌面"分叉规则在两个入口漂移。
//! 两端 serve 同一 `remote-dist` 产物根，分叉决策亦共用一份。

/// 是否优先发桌面 SPA：仅当 URL 显式带 `?ui=desktop` 时返回 `true`；其余
/// （无 override / `?ui=mobile` / 任意 UA / 任意窗口宽度 / 任意历史偏好）一律 `false`。
///
/// 之所以不再嗅探 UA：电脑浏览器访问 Remote 默认也得是手机端 UI——它在
/// 桌面尺寸下仍能用（实体键盘、宽度自适应），同时保住单一实现路径，便于
/// 桌面 Web Remote 与原生 Ridge Desktop 的功能复用。`?ui=desktop` 是唯一
/// 切到桌面 SPA 的入口，原生 Ridge Desktop 不受影响（那是 Tauri shell，
/// 不走这条 serve 链路）。
///
/// 注意：本函数只做"想要哪套 UI"的判定；调用方仍需校验对应产物目录是否存在
/// （桌面产物缺失时应自行回退到移动 SPA）。
pub fn prefer_desktop_ui(_ua: &str, ui_override: Option<&str>) -> bool {
    matches!(ui_override, Some("desktop"))
}

/// 解析 URL 查询 `ui` 参数；用于把原始 HTTP 查询参数收成统一格式。
///
/// 只认 `?ui=desktop` / `?ui=mobile` 两个合法值；其它（含 `?ui=foo`）一律视为
/// `None`——SSOT 之外的探测入口必须显式可控，不能因为用户手滑拼错就把桌面端
/// 误升上去。
pub fn parse_ui_override(raw: Option<&str>) -> Option<&str> {
    match raw {
        Some("desktop") => Some("desktop"),
        Some("mobile") => Some("mobile"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_mobile_regardless_of_ua() {
        // 任何 UA 在没有 `?ui=desktop` 时都必须落到移动 SPA。
        for ua in [
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Safari",
            "Mozilla/5.0 (Linux; Android 14; Pixel 8) Chrome Mobile",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/120 Safari",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) Safari",
            "",
        ] {
            assert!(
                !prefer_desktop_ui(ua, None),
                "无覆盖时所有 UA 都应是 mobile：{ua}"
            );
            assert!(
                !prefer_desktop_ui(ua, Some("mobile")),
                "?ui=mobile 必须尊重：{ua}"
            );
        }
    }

    #[test]
    fn only_explicit_desktop_override_forces_desktop() {
        for ua in [
            "Mozilla/5.0 (iPhone)",
            "Mozilla/5.0 (Windows NT 10.0)",
            "Mozilla/5.0 (Linux; Android 14)",
            "",
        ] {
            assert!(
                prefer_desktop_ui(ua, Some("desktop")),
                "?ui=desktop 必须切桌面：{ua}"
            );
        }
    }

    #[test]
    fn garbage_ui_values_do_not_force_desktop() {
        // `?ui=foo` / `?ui=DESKTOP`（大小写敏感）等不在白名单的值必须视同无覆盖，
        // 否则用户手滑就把桌面 SPA 升上去，反而绕过了修复。
        for bad in ["foo", "DESKTOP", "Desktop", "desktop ", " desktop", "true"] {
            assert!(
                !prefer_desktop_ui("Mozilla/5.0 (Windows)", Some(bad)),
                "垃圾 ui 值 {bad:?} 不该切桌面"
            );
        }
    }

    #[test]
    fn parse_ui_override_whitelist() {
        assert_eq!(parse_ui_override(Some("desktop")), Some("desktop"));
        assert_eq!(parse_ui_override(Some("mobile")), Some("mobile"));
        assert_eq!(parse_ui_override(Some("foo")), None);
        assert_eq!(parse_ui_override(None), None);
        assert_eq!(parse_ui_override(Some("")), None);
    }
}
