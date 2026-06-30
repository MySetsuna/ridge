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

// ─── ToolRegistry ────────────────────────────────────────────────────────────

/// Ridge MCP 工具注册表。
///
/// `Default::default()` 预注册五个内置工具。可调用 `register` 追加自定义工具。
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
                description: "向指定 pane 的 teammate 发送文本消息。".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "target_pane_id": {
                            "type": "number",
                            "description": "目标 pane 的数字 ID"
                        },
                        "message": {
                            "type": "string",
                            "description": "要发送的消息内容"
                        }
                    },
                    "required": ["target_pane_id", "message"]
                }),
            },
            ToolSpec {
                name: "ridge_delegate_task".to_string(),
                description: "将一个多步骤任务委派给指定 pane 的 teammate 执行。".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "target_pane_id": {
                            "type": "number",
                            "description": "负责执行任务的 pane ID"
                        },
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
                name: "ridge_stash_data".to_string(),
                description: "将 base64 编码的内容存入 ridge:// 内存中转站，返回 URI。".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "content_base64": {
                            "type": "string",
                            "description": "要暂存的内容（base64 编码）"
                        }
                    },
                    "required": ["content_base64"]
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
    fn default_registry_has_five_tools() {
        let reg = ToolRegistry::default();
        assert_eq!(reg.tools().len(), 5);
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
        assert_eq!(reg.tools().len(), 6);
        assert!(reg.get("custom_tool").is_some());
    }

    #[test]
    fn tools_list_result_has_tools_key() {
        let reg = ToolRegistry::default();
        let v = reg.tools_list_result();
        assert!(v["tools"].is_array());
        assert_eq!(v["tools"].as_array().unwrap().len(), 5);
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
    fn ridge_stash_data_requires_content_base64() {
        let reg = ToolRegistry::default();
        let spec = reg.get("ridge_stash_data").unwrap();
        let required = spec.input_schema["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(names.contains(&"content_base64"));
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
        // 缺口3 回归守卫：`tools/call`（src-tauri/teammate/server.rs::mcp_tools_call）
        // 路由这三个工具。它们必须出现在 `tools/list` 里，否则 agent 发现得到却调用即
        // "unknown tool"，自由交流链路断。
        let reg = ToolRegistry::default();
        for name in [
            "ridge_send_to_teammate",
            "ridge_delegate_task",
            "ridge_get_team_profile",
        ] {
            assert!(
                reg.get(name).is_some(),
                "routed tool {name} missing from tools/list registry"
            );
        }
    }
}
