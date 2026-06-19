//! Domain A2 —— Teammate Markup Language (TML)。
//!
//! 当多个 Agent 之间没有 MCP 协议可用时，它们通过向彼此的 PTY 写入**结构化文本**
//! 进行「在野」社交。一个 TML 块形如：
//!
//! ```text
//! @@RIDGE_TML_START@@
//! {"version":"1.0","msg_id":"<uuid>","from_pane":"pane_01","to_pane":"pane_02",
//!  "action":{"type":"AssignTask","payload":{"objective":"run tests","max_steps":20}},
//!  "task_id":null}
//! <body text，可多行>
//! @@RIDGE_TML_END@@
//! ```
//!
//! 本文件只承载**类型 + 纯解析辅助**；真正的字节级流式状态机（跨 chunk 边界、
//! MUTATION_HIDE 隐藏、回显抑制）在同目录 [`super::stream_cleaner`]。

use serde::{Deserialize, Serialize};

/// TML 块起始标记（含结尾换行，便于状态机按行锁定）。
pub const TML_START: &str = "@@RIDGE_TML_START@@\n";
/// TML 块结束标记（含结尾换行）。
pub const TML_END: &str = "@@RIDGE_TML_END@@\n";

/// 内联控制动作。线上以 `{"type": "...", "payload": {...}}` 形式承载
/// （`PeerTalk` 无 payload）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum TmlAction {
    /// 纯文本多模态对话 / 闲聊。
    PeerTalk,
    /// 派发一个具体物理子任务。
    AssignTask { objective: String, max_steps: u32 },
    /// 权限转交（老大挂起、小弟取得输入焦点）。
    YieldControl { reason: String },
    /// 子智能体向主智能体反馈阶段性结果。
    ReportStatus { status: String, exit_code: i32 },
}

/// TML 报文头部：描述路由与动作语义的严苛 JSON 结构。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TmlHeader {
    /// 固定 "1.0"。
    pub version: String,
    /// UUIDv4，区分会话。
    pub msg_id: String,
    /// 发起方 Pane ID。
    pub from_pane: String,
    /// 目标方 Pane ID。
    pub to_pane: String,
    /// 内联控制动作。
    pub action: TmlAction,
    /// 级联任务链 ID（可空）。
    pub task_id: Option<String>,
}

impl TmlHeader {
    /// 构造一个新头部：自动填 `version="1.0"` 与全新 uuid v4 的 `msg_id`，
    /// `task_id` 为空。
    pub fn new(
        from_pane: impl Into<String>,
        to_pane: impl Into<String>,
        action: TmlAction,
    ) -> Self {
        Self {
            version: "1.0".to_string(),
            msg_id: uuid::Uuid::new_v4().to_string(),
            from_pane: from_pane.into(),
            to_pane: to_pane.into(),
            action,
            task_id: None,
        }
    }
}

/// 一条**完整解析出**的 TML 消息（头部 + 正文体）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TmlMessage {
    pub header: TmlHeader,
    pub body: String,
}

impl TmlMessage {
    pub fn new(header: TmlHeader, body: impl Into<String>) -> Self {
        Self {
            header,
            body: body.into(),
        }
    }

    /// 渲染为完整线上块（START + 头部 JSON 行 + 正文 + 单个换行 + END）。
    /// 主要供测试与未来的 TML 写出器复用。约定：正文与 END 之间**恒**插入恰好
    /// 一个 `\n`（解析侧 [`super::stream_cleaner`] 也恒剥一个尾随 `\n`），保证
    /// 任意正文（含以 `\n` 结尾者）都能精确 roundtrip。
    pub fn encode(&self) -> String {
        let header_json = serde_json::to_string(&self.header).unwrap_or_default();
        format!("{TML_START}{header_json}\n{}\n{TML_END}", self.body)
    }
}

/// TML 解析错误。
#[derive(Debug, thiserror::Error)]
pub enum TmlError {
    /// 头部 JSON 反序列化失败（降级时回吐原文，不应挂死调用方）。
    #[error("TML 头部 JSON 解析失败: {0}")]
    HeaderJson(#[from] serde_json::Error),
}

/// 解析一行（不含尾换行）TML 头部 JSON。
pub fn parse_header(line: &[u8]) -> Result<TmlHeader, TmlError> {
    Ok(serde_json::from_slice::<TmlHeader>(line)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_serde_tagged_roundtrip() {
        let a = TmlAction::AssignTask {
            objective: "run tests".into(),
            max_steps: 20,
        };
        let s = serde_json::to_string(&a).unwrap();
        assert!(s.contains("\"type\":\"AssignTask\""), "got {s}");
        assert!(s.contains("\"payload\""), "got {s}");
        let back: TmlAction = serde_json::from_str(&s).unwrap();
        assert_eq!(a, back);
    }

    #[test]
    fn peer_talk_has_no_payload_body() {
        let a = TmlAction::PeerTalk;
        let s = serde_json::to_string(&a).unwrap();
        // 无 payload 内容时 serde 仍可 roundtrip。
        let back: TmlAction = serde_json::from_str(&s).unwrap();
        assert_eq!(a, back);
    }

    #[test]
    fn header_new_fills_version_and_uuid() {
        let h = TmlHeader::new("pane_01", "pane_02", TmlAction::PeerTalk);
        assert_eq!(h.version, "1.0");
        assert_eq!(h.from_pane, "pane_01");
        assert_eq!(h.to_pane, "pane_02");
        assert!(h.task_id.is_none());
        // uuid v4 形如 8-4-4-4-12。
        assert_eq!(h.msg_id.len(), 36);
        assert_eq!(h.msg_id.matches('-').count(), 4);
    }

    #[test]
    fn header_json_roundtrip() {
        let h = TmlHeader {
            version: "1.0".into(),
            msg_id: "abc".into(),
            from_pane: "p1".into(),
            to_pane: "p2".into(),
            action: TmlAction::ReportStatus {
                status: "ok".into(),
                exit_code: 0,
            },
            task_id: Some("tsk_1".into()),
        };
        let line = serde_json::to_vec(&h).unwrap();
        let back = parse_header(&line).unwrap();
        assert_eq!(h, back);
    }

    #[test]
    fn encode_then_parse_header_roundtrips() {
        let header = TmlHeader::new("p1", "p2", TmlAction::PeerTalk);
        let msg = TmlMessage::new(header.clone(), "hello\nworld");
        let wire = msg.encode();
        assert!(wire.starts_with(TML_START));
        assert!(wire.ends_with(TML_END));
        // 取出头部 JSON 行（START 与第一个换行之间）。
        let after_start = &wire[TML_START.len()..];
        let nl = after_start.find('\n').unwrap();
        let header_line = &after_start[..nl];
        let parsed = parse_header(header_line.as_bytes()).unwrap();
        assert_eq!(parsed, header);
    }

    #[test]
    fn malformed_header_errs() {
        let bad = b"{not valid json";
        assert!(parse_header(bad).is_err());
    }
}
