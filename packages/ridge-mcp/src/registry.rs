//! Domain C1 — MCP Tool 注册表（ToolSpec + tools/list 序列化）。由 Phase 1 / TM-C 填充。

use serde::{Deserialize, Serialize};

// ─── ToolSpec ────────────────────────────────────────────────────────────────

/// 一条 MCP 工具规格，对应 tools/list 响应中的单个条目。
///
/// `input_schema` 字段在 wire 上序列化为 `inputSchema`（MCP 规范要求）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// pane 寻址参数的统一 schema：花名册回传的 `paneId`（Uuid 串）与 `paneIndex`（数字）
/// 都合法。旧规格只写 `"type": "number"`，按规格生成参数的客户端拿不到 Uuid 这条路。
fn pane_target_schema() -> serde_json::Value {
    serde_json::json!({
        "type": ["string", "number"],
        "description": "目标 pane：花名册的 paneId（Uuid 串，推荐）或 paneIndex（数字索引）"
    })
}

// ─── ToolRegistry ────────────────────────────────────────────────────────────

/// Ridge MCP 工具注册表。
///
/// `Default::default()` 预注册内置工具。可调用 `register` 追加自定义工具。
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    tools: Vec<ToolSpec>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        let tools = vec![
            ToolSpec {
                name: "ridge_split_pane".to_string(),
                description: "在终端工作区分割出一个新 pane，指定方向和初始角色。".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "direction": {
                            "type": "string",
                            "enum": ["horizontal", "vertical"],
                            "description": "分割方向"
                        },
                        "role": {
                            "type": "string",
                            "description": "新 pane 的角色标识（如 worker / reviewer）"
                        },
                        "initial_cmd": {
                            "type": "string",
                            "description": "新 pane 启动后立即执行的命令（可选）"
                        }
                    },
                    "required": ["direction", "role"]
                }),
            },
            ToolSpec {
                name: "ridge_send_to_teammate".to_string(),
                description:
                    "向指定 pane 写入文本、默认派发 Enter，并留一份到收件箱；仅 submit=false 时注入草稿。"
                        .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "target_pane_id": pane_target_schema(),
                        "message": {
                            "type": "string",
                            "description": "要发送的消息内容"
                        },
                        "from": {
                            "type": "string",
                            "description": "发送方标识（跨 agent 协作时写自己的名字，默认 mcp-client）"
                        },
                        "submit": {
                            "type": "boolean",
                            "description": "是否派发 Enter；默认 true，仅显式 false 时保留为草稿"
                        }
                    },
                    "required": ["target_pane_id", "message"]
                }),
            },
            ToolSpec {
                name: "ridge_delegate_task".to_string(),
                description: "将一个多步骤任务委派给指定 pane 并显式派发 Enter；返回 submit_dispatched，不代表 Agent 已执行。".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "target_pane_id": pane_target_schema(),
                        "objective": {
                            "type": "string",
                            "description": "任务目标描述"
                        },
                        "max_steps": {
                            "type": "number",
                            "description": "允许的最大执行步骤数"
                        }
                    },
                    "required": ["target_pane_id", "objective"]
                }),
            },
            ToolSpec {
                name: "ridge_send_and_submit".to_string(),
                description: "向目标 pane 写入文本并显式派发 Enter。回执另列 terminalAccepted；二者均不代表 Agent 已执行。".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "target_pane_id": pane_target_schema(),
                        "message": { "type": "string", "description": "要写入并提交的文本" },
                        "from": { "type": "string", "description": "发送方标识" }
                    },
                    "required": ["target_pane_id", "message"]
                }),
            },
            ToolSpec {
                name: "ridge_delivery_status".to_string(),
                description: "读取指定投递回执。submit_dispatched 只表示已派发 Enter；terminalAccepted 与 agentAcknowledged 分别独立报告。".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "target_pane_id": pane_target_schema(),
                        "receipt_id": { "type": "string", "description": "发送工具返回的 receiptId" }
                    },
                    "required": ["target_pane_id", "receipt_id"]
                }),
            },
            ToolSpec {
                name: "ridge_acknowledge_receipt".to_string(),
                description: "目标 Agent 明确确认或拒绝一条已投递输入；仅此工具可把 agentAcknowledged 置真。".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "target_pane_id": pane_target_schema(),
                        "receipt_id": { "type": "string", "description": "收到消息中的 receiptId" },
                        "status": { "type": "string", "enum": ["agent_acknowledged", "agent_rejected"] },
                        "detail": { "type": "string", "description": "可选的确认或拒绝原因" }
                    },
                    "required": ["target_pane_id", "receipt_id", "status"]
                }),
            },
            ToolSpec {
                name: "ridge_report_execution_rejection".to_string(),
                description: "上报外部执行网关的拒绝，生成含执行者、策略来源、请求 ID 与替代步骤的 Ridge 桌面卡片。只记录/展示；绝不表示 Ridge 可重试外部命令。".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "initiator": { "type": "string", "description": "发起该外部操作的 agent/客户端" },
                        "action": { "type": "string", "description": "可选：拒绝的命令或安全摘要；不得包含密钥" },
                        "executor": { "type": "string", "description": "实际拒绝方，例如 Codex execution gateway" },
                        "policy_source": { "type": "string", "description": "策略来源，例如组织执行策略" },
                        "request_id": { "type": "string", "description": "外部拒绝返回的 request ID" },
                        "reason": { "type": "string", "description": "拒绝原因/错误原文摘要" },
                        "next_step": { "type": "string", "description": "用户可执行的替代步骤；不得声称 Ridge 可以重试" }
                    },
                    "required": ["executor", "policy_source", "request_id", "reason", "next_step"]
                }),
            },
            ToolSpec {
                name: "ridge_stash_data".to_string(),
                description:
                    "把一段文本存进 ridge:// 内存中转站，返回 ridge://cache/<id>，供其它 agent resources/read 回读。"
                        .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "data": {
                            "type": "string",
                            "description": "要暂存的纯文本内容"
                        }
                    },
                    "required": ["data"]
                }),
            },
            ToolSpec {
                name: "ridge_capture_pane".to_string(),
                description: "抓取指定 pane 的当前屏幕文本（已渲染，非转义序列），用于观察队友进展。"
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "target_pane_id": pane_target_schema(),
                        "lines": {
                            "type": "number",
                            "description": "取末 N 行（默认 80，上限 2000）"
                        }
                    },
                    "required": ["target_pane_id"]
                }),
            },
            ToolSpec {
                name: "ridge_inbox_read".to_string(),
                description:
                    "取走投递给某个 pane 的消息（跨 agent 异步回话通道；stdin 注入之外的可靠副本）。"
                        .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "target_pane_id": pane_target_schema(),
                        "peek": {
                            "type": "boolean",
                            "description": "true 只窥视不清空（默认 false：取走即清空）"
                        }
                    },
                    "required": ["target_pane_id"]
                }),
            },
            ToolSpec {
                name: "ridge_report_progress".to_string(),
                description: "向工作区回流一条进展（worker 主动汇报，落前端进度事件）。".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "target_pane_id": pane_target_schema(),
                        "status": {
                            "type": "string",
                            "description": "状态标识，如 working / blocked / done"
                        },
                        "detail": {
                            "type": "string",
                            "description": "一句话说明"
                        }
                    },
                    "required": ["target_pane_id", "status"]
                }),
            },
            ToolSpec {
                name: "ridge_get_team_profile".to_string(),
                description: "获取当前工作区所有 teammate pane 的身份与状态快照。".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolSpec {
                name: "ridge_join_group".to_string(),
                description:
                    "把一个 teammate（按 agent_id 或 pane）加入某个按名字寻址的已有编组。"
                        .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "group_name": {
                            "type": "string",
                            "description": "目标编组的名称（前端花名册里的组名，同名取首个）"
                        },
                        "agent_id": {
                            "type": "string",
                            "description": "要加入的成员 agent_id（花名册 id；与 target_pane_id 二选一）"
                        },
                        "target_pane_id": {
                            "description": "要加入的成员 pane（Uuid 串或数字索引；后端反查其 agent_id）"
                        }
                    },
                    "required": ["group_name"]
                }),
            },
        ];
        Self { tools }
    }
}

impl ToolRegistry {
    /// 返回所有已注册工具的切片。
    pub fn tools(&self) -> &[ToolSpec] {
        &self.tools
    }

    /// 按名称查找工具，返回 `None` 表示未注册。
    pub fn get(&self, name: &str) -> Option<&ToolSpec> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// 追加一个自定义工具规格。
    pub fn register(&mut self, spec: ToolSpec) {
        self.tools.push(spec);
    }

    /// 序列化为 tools/list 的 result payload：`{"tools": [...]}`.
    pub fn tools_list_result(&self) -> serde_json::Value {
        serde_json::json!({ "tools": self.tools })
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_has_thirteen_tools() {
        let reg = ToolRegistry::default();
        assert_eq!(reg.tools().len(), 13);
    }

    #[test]
    fn get_returns_known_tool() {
        let reg = ToolRegistry::default();
        let spec = reg.get("ridge_split_pane").unwrap();
        assert_eq!(spec.name, "ridge_split_pane");
    }

    #[test]
    fn get_returns_none_for_unknown_tool() {
        let reg = ToolRegistry::default();
        assert!(reg.get("nonexistent_tool").is_none());
    }

    #[test]
    fn register_appends_custom_tool() {
        let mut reg = ToolRegistry::default();
        reg.register(ToolSpec {
            name: "custom_tool".to_string(),
            description: "test".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
        });
        assert_eq!(reg.tools().len(), 14);
        assert!(reg.get("custom_tool").is_some());
    }

    #[test]
    fn tools_list_result_has_tools_key() {
        let reg = ToolRegistry::default();
        let v = reg.tools_list_result();
        assert!(v["tools"].is_array());
        assert_eq!(v["tools"].as_array().unwrap().len(), 13);
    }

    #[test]
    fn input_schema_serializes_as_camel_case() {
        let reg = ToolRegistry::default();
        let v = reg.tools_list_result();
        let first = &v["tools"][0];
        // wire field must be "inputSchema", not "input_schema"
        assert!(first.as_object().unwrap().contains_key("inputSchema"));
        assert!(!first.as_object().unwrap().contains_key("input_schema"));
    }

    #[test]
    fn ridge_split_pane_requires_direction_and_role() {
        let reg = ToolRegistry::default();
        let spec = reg.get("ridge_split_pane").unwrap();
        let required = spec.input_schema["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(names.contains(&"direction"));
        assert!(names.contains(&"role"));
    }

    #[test]
    fn ridge_get_team_profile_has_empty_required() {
        let reg = ToolRegistry::default();
        let spec = reg.get("ridge_get_team_profile").unwrap();
        let required = spec.input_schema["required"].as_array().unwrap();
        assert!(required.is_empty());
    }

    #[test]
    fn ridge_stash_data_requires_data() {
        // 规格曾写 content_base64、实现却读 data —— 客户端按规格调用必失败。统一为 data。
        let reg = ToolRegistry::default();
        let spec = reg.get("ridge_stash_data").unwrap();
        let required = spec.input_schema["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(names.contains(&"data"));
    }

    #[test]
    fn pane_target_schema_accepts_uuid_and_index() {
        let reg = ToolRegistry::default();
        for name in [
            "ridge_send_to_teammate",
            "ridge_send_and_submit",
            "ridge_delegate_task",
            "ridge_capture_pane",
            "ridge_inbox_read",
            "ridge_delivery_status",
            "ridge_acknowledge_receipt",
        ] {
            let t = &reg.get(name).unwrap().input_schema["properties"]["target_pane_id"]["type"];
            let kinds: Vec<&str> = t.as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
            assert!(kinds.contains(&"string") && kinds.contains(&"number"), "{name} 寻址类型过窄");
        }
    }

    #[test]
    fn ridge_delegate_task_requires_target_and_objective() {
        let reg = ToolRegistry::default();
        let spec = reg.get("ridge_delegate_task").unwrap();
        let required = spec.input_schema["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(names.contains(&"target_pane_id"));
        assert!(names.contains(&"objective"));
    }

    #[test]
    fn routed_tools_are_advertised() {
        // 回归守卫：`tools/call`（ridge_core::mcp::server::tools_call）路由这些工具。
        // 它们必须出现在 `tools/list` 里，否则 agent 发现得到却调用即 "unknown tool"，
        // 自由交流链路断。反向守卫（广告了必须能调）在 server.rs 的
        // `every_advertised_tool_is_routed`。
        let reg = ToolRegistry::default();
        for name in [
            "ridge_send_to_teammate",
            "ridge_send_and_submit",
            "ridge_delegate_task",
            "ridge_get_team_profile",
            "ridge_join_group",
            "ridge_split_pane",
            "ridge_capture_pane",
            "ridge_inbox_read",
            "ridge_delivery_status",
            "ridge_acknowledge_receipt",
            "ridge_report_execution_rejection",
            "ridge_report_progress",
            "ridge_stash_data",
        ] {
            assert!(
                reg.get(name).is_some(),
                "routed tool {name} missing from tools/list registry"
            );
        }
    }

    #[test]
    fn ridge_join_group_requires_group_name() {
        let reg = ToolRegistry::default();
        let spec = reg.get("ridge_join_group").unwrap();
        let required = spec.input_schema["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(names, vec!["group_name"]);
        // agent_id / target_pane_id 是二选一的可选项，不在 required 里。
        let props = spec.input_schema["properties"].as_object().unwrap();
        assert!(props.contains_key("agent_id"));
        assert!(props.contains_key("target_pane_id"));
    }
}
