# NotebookLM Guidance · Iteration 64

日期：2026-07-28
需求：`REQ-AGENT-HISTORY-01`（用户自动批准）
来源：覆盖更新后的 `PROJECT-STATE.md` 与 `REQUIREMENTS-SPEC.md`

## 首轮建议

- Agent 侧栏顶级导航改“成员 / 编组 / 历史”，历史按 Agent 类型分组折叠。
- 删 `recentReplies` 独立展示，复用 `project.rs` 现有 Claude/Codex/OpenCode 有界读取与 `RidgePane` 进程识别。
- 以单一 adapter 描述 identity、process evidence、session discovery、title、resume capability、结构化 launch。
- 恢复在当前工作区新建 pane；不写第三方 session，不上传内容。

## 对抗评审

打回：

- “能读 JSONL 即可恢复”：历史内容不证明原启动参数或官方 resume 契约。
- “Aider 已识别即可恢复”：进程识别不证明 session/resume 能力。
- 新建持久 history store 与大目录抽象：首版应复用 `project.rs`，折叠态留组件内 UI state。

修订：

- adapter 的 discovery 与 resume 能力分离；无本机可执行文件及精确 argv 证据者，仍可发现但恢复禁用。
- 仅结构化 `{ executable, argv, cwd }` 可进入 pane 创建链；禁止 shell 字符串拼接。
- 单 adapter 解析失败隔离；未知/损坏 session 有确定性回退。
- 无已安装真 CLI 时，E2E 只验证禁用降级；不得宣称该 CLI 恢复通过。
- 历史 session 若以原生 session ID 命中 Ridge 当前运行项，须复用“成员/编组”同款交互 Agent 项；无稳定 ID 不以标题/cwd 猜合并。

## 裁决

通过“证据先行”三切片：

1. 有界只读发现与统一 DTO；
2. adapter 证据表及 capability 分级；
3. UI 分组折叠 + 经证据核准的结构化恢复。

MiMo、OpenCode、Grok 与中国主流 CLI 均先做证据盘点；只有 discovery 证据便只列历史，只有明确 resume 契约及本机验证方开放按钮。
