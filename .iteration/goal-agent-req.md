### REQ-AGENT-CATALOG-01 · Agent 识别表 / 历史 / 恢复 YOLO / 设置

- 状态:`ACTIVE`
- 版本:`v1`
- 行为:运行中 Grok 进程须被 agent 发现路径识别；历史须提取 Codex 与 Grok 会话；点击恢复须切 cwd 并自动 resume；恢复旁 YOLO 开关按 agent 配置注入 yolo 参数；设置-智能体可配置进程名/启动/yolo 参数；内置 claude/codex/grok 等默认，识别以该表为准。
- 边界:不伪造 NLM 内容；不自动发版；冷门 agent 靠用户自定义。
- 验收:cargo test agent_catalog/parses_grok/parses_codex 绿；KNOWN_AGENT_NAMES 含 grok；plan_agent_resume 可 YOLO。
- 追踪:`REQ-AGENT-CATALOG-01` → teammate/agent_catalog.rs → project.rs history → AgentCenterPanel/SettingsPanel
