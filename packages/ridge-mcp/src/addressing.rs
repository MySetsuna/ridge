//! Domain C — MCP `tools/call` pane 寻址解析（缺口1 寻址自洽的纯逻辑核心）。
//!
//! 花名册资源 `ridge://workspace/active-panes` 同时暴露 `paneId`(Uuid 字符串) 与
//! `paneIndex`(叶子数字索引)。本模块把工具入参里的 `target_pane_id` 解析为
//! [`PaneTarget`]，让两端任选其一都能寻址。把这层放在 `ridge-core` 是因为它是
//! 纯函数、无 `AppState` 依赖、可 `cargo test -p ridge-core` 独立验证（ridge cdylib
//! 的 `--lib` 单测在本机跑不起来）。Uuid 是否属于当前工作区的校验在传输层
//! （`src-tauri/teammate/server.rs`）完成，本模块只做语法解析。

use uuid::Uuid;

/// `target_pane_id` 的解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneTarget {
    /// 直接给出的 pane Uuid（来自花名册 `paneId`）。上层需校验它属于当前工作区。
    Uuid(Uuid),
    /// 叶子索引（来自花名册 `paneIndex`，或历史数字契约）。
    Index(usize),
}

/// 把 `tools/call` 参数里的 `target_pane_id` 解析为 [`PaneTarget`]。
///
/// 兼容三种写法（缺口1 寻址自洽）：
/// - Uuid 字符串（如 `"3f2504e0-4f89-41d3-9a0c-0305e82c3301"`）→ [`PaneTarget::Uuid`]
/// - 数字（如 `2`）→ [`PaneTarget::Index`]
/// - 纯数字字符串（如 `"2"`）→ [`PaneTarget::Index`]（容错：agent 把索引字符串化）
///
/// 其它（缺失 / 布尔 / 浮点 / 负数 / 非法串）→ `Err(String)`。
/// 解析顺序「先 Uuid 再数字」保证形如 `"12345678-..."` 的 Uuid 不会被误判为索引。
pub fn parse_pane_target(value: &serde_json::Value) -> Result<PaneTarget, String> {
    if let Some(s) = value.as_str() {
        let s = s.trim();
        if let Ok(u) = Uuid::parse_str(s) {
            return Ok(PaneTarget::Uuid(u));
        }
        if let Ok(idx) = s.parse::<usize>() {
            return Ok(PaneTarget::Index(idx));
        }
        return Err(format!("target_pane_id 既不是 Uuid 也不是数字索引: {s:?}"));
    }
    if let Some(idx) = value.as_u64() {
        return Ok(PaneTarget::Index(idx as usize));
    }
    Err("target_pane_id 必须是 pane Uuid 字符串或数字索引".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_uuid_string() {
        let id = Uuid::new_v4();
        let v = json!(id.to_string());
        assert_eq!(parse_pane_target(&v).unwrap(), PaneTarget::Uuid(id));
    }

    #[test]
    fn parses_uuid_string_with_surrounding_whitespace() {
        let id = Uuid::new_v4();
        let v = json!(format!("  {id}  "));
        assert_eq!(parse_pane_target(&v).unwrap(), PaneTarget::Uuid(id));
    }

    #[test]
    fn parses_numeric_index() {
        assert_eq!(parse_pane_target(&json!(2)).unwrap(), PaneTarget::Index(2));
    }

    #[test]
    fn parses_zero_index() {
        assert_eq!(parse_pane_target(&json!(0)).unwrap(), PaneTarget::Index(0));
    }

    #[test]
    fn parses_numeric_string_as_index() {
        assert_eq!(
            parse_pane_target(&json!("3")).unwrap(),
            PaneTarget::Index(3)
        );
    }

    #[test]
    fn rejects_garbage_string() {
        assert!(parse_pane_target(&json!("not-a-pane")).is_err());
    }

    #[test]
    fn rejects_null() {
        assert!(parse_pane_target(&serde_json::Value::Null).is_err());
    }

    #[test]
    fn rejects_bool() {
        assert!(parse_pane_target(&json!(true)).is_err());
    }

    #[test]
    fn rejects_negative_number() {
        // -1 is i64, not u64 → as_u64() is None → not a valid index.
        assert!(parse_pane_target(&json!(-1)).is_err());
    }

    #[test]
    fn rejects_fractional_number() {
        // 2.5 must not silently truncate to an index.
        assert!(parse_pane_target(&json!(2.5)).is_err());
    }
}
