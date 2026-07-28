# Contract · Iteration 64 · Agent 历史会话

状态：APPROVED
日期：2026-07-28
需求：`REQ-AGENT-HISTORY-01`

## 目标

Agent Center 顶级页签统一为“成员 / 编组 / 历史”。历史按 Agent 类型分组，可单组或全部折叠/展开；每项列稳定标题、时间、cwd，并在有可靠契约时恢复至当前工作区的新 pane。

## 切片 1 · 有界发现与 DTO

- 复用 `src-tauri/src/commands/project.rs` 现有历史读取；删除 `recentReplies` 独立投影。
- 统一只读 DTO：`agentId/sessionId/title/updatedAt/cwd/source/canResume/resumeUnavailableReason`。
- 单 adapter 限文件数、单文件读取量、总条数；损坏/漂移仅隔离该 adapter 或条目。
- 标题只取原生标题或短确定性回退，不取大段正文。

硬门：fixture 覆盖 Claude/Codex/OpenCode、损坏文件、未知类型、上限；不改写第三方文件。

## 切片 2 · Adapter 证据表

- 单一 registry 复用运行中 identity 与历史发现，明确分离 `processEvidence`、`discovery`、`resume`。
- 先盘点本机 executable、session 路径/格式及官方 `--help`/文档证据，再纳入 MiMo/OpenCode/Grok 及中国主流 CLI。
- 无 discovery 证据者不得伪造历史；无 resume 证据或 executable 不存在者 `canResume=false`。
- Aider 等仅有进程识别者不得自动升级为可恢复。

硬门：每个 adapter 具证据注释/fixture；模糊进程名冲突归“其他”；单个 adapter 失败不拖垮列表。

停止条件：须猜 session 路径/参数、解析需上传内容、或第三方格式无法有界读取。

## 切片 3 · UI 与结构化恢复

- `AgentCenterPanel.svelte` 顶级页签恰为“成员 / 编组 / 历史”；“最近回复”入口消失。
- 组级折叠互不影响；全部折叠/展开；状态只在组件/UI store，不写 `.ridge` 或第三方 session。
- 历史 DTO 与运行中 roster/native session 以 adapter 原生 session ID 归并；命中者复用成员/编组现有交互项，不重复渲染静态历史行。无稳定 ID 时保持两项，禁止凭标题/cwd 猜合并。
- 恢复单飞；当前工作区新建一个 pane，不改旧 pane/session。
- pane 创建仅收结构化 `executable + argv[] + cwd`，Rust 端不经 shell 解释；工作目录 canonicalize，参数原样传入。
- 失败保留 session，并在新 pane/操作结果中可诊断。

硬门：

- 组件测试覆盖三组混排、组/全部折叠、键盘/a11y、禁用原因、重复点击，以及运行中替换/退出回退/同名不误并。
- Rust 测捕获 executable/argv/cwd，含恶意参数 fixture，证明无 shell 注入。
- 每个宣称可恢复 adapter 需真 CLI 或等价真实进程 E2E；未安装者只验诚实禁用。
- `pnpm test`、`pnpm check`、Rust lib、production build、Sonar changed-code 信号均绿。

## 允许路径

- `src/lib/teammate/AgentCenterPanel.svelte`
- `src/lib/components/RidgePane.svelte` 中既有 agent identity 识别的归口点
- `src-tauri/src/commands/project.rs`
- pane 创建/PTY spawn 的既有结构化调用链
- 对应最小 fixture 与测试

## 禁止路径

- ridge-cloud、Remote 协议与 allowlist
- 第三方 session 内容写入、迁移或上传
- 新持久数据库/history cache
- shell command 字符串拼接
- 无证据 CLI 的猜测性 resume

## 追踪

| 需求 | 代码 | 测试 |
| --- | --- | --- |
| 三顶级页签、分组折叠 | `AgentCenterPanel.svelte` | 组件交互/a11y |
| 稳定标题、有界发现 | `project.rs` + adapter registry | JSONL/损坏/上限 fixture |
| CLI 扩展 | identity 归口 + adapter evidence | process/session fixtures |
| 运行中历史复用交互项 | history↔roster 原生 session ID join | 命中替换、退出回退、同名隔离 |
| 当前区新 pane 恢复 | pane create / PTY spawn | argv 安全测 + 真进程 E2E |
