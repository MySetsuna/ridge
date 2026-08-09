# 2026-08-10 迭代总交接：Agent 通信架构与质量覆盖

日期：2026-08-10

## 需求与来源

- 基线笔记：`Ridge 项目现状、愿景与规划基线（2026-07-21）`，深化主题为 `Agent 通信架构重构`。
- 等权来源：NotebookLM source `9516749e-c317-4f13-9cda-b64b00cec465`；临时对话 source `df4d5dcc-9813-4c61-ae9f-1e9199cb7555`。
- 临时对话新增边界已纳入 `REQ-AGENT-COMMUNICATION-ARCH-REBUILD-01`：Cookie/API 认证边界、MIME/大小/SHA256 校验、重试/轮询上限、token/cookie 脱敏、取消传播。
- 本轮需求 gate：`INTAKE-20260809-ITERATION-CONTINUE-01`，`executable=true`，`pending_ids=[]`，`active_missing_headings=[]`。
- 在途需求：`REQ-NLM-ITERATION-01`、`REQ-NLM-CLOSURE-20260809`、`REQ-AGENT-COMMUNICATION-REGISTRY-01`、`REQ-AGENT-COMMUNICATION-ARCH-REBUILD-01`、`REQ-SONAR-COVERAGE-80-01`。

## 本轮已提交

- `7e7a329a`：LAN Remote Agent Hub receipt、scrollback malformed/stale commit 边界。
- `64bd79ca`：fileEditor Tauri/浏览器错误边界。
- `a66eda86`：ridgeCloudProvider 会话、ICE、Pane/control 通道与信令生命周期。
- `14e2c7b5`：fileExplorer 分页、浏览器降级、stale path 与折叠投影。
- `6b40335a`：wsRemote 能力降级、空输入、坏事件与 `4403 DEVICE_PARKED` 分级。
- 每波均含对应 `docs/iterations/2026-08-10-wave*-handoff.md` 与 `docs/REQUIREMENTS-SPEC.md` 证据；仅提交测试/文档，未改生产通信语义。

## 当前质量证据

- 聚焦回归：wsRemote `7/7`、fileEditor `13/13`、ridgeCloudProvider `19/19`、fileExplorer `46/46`。
- 全量 `pnpm test:coverage:sonar`：exit `0`，`normalize-lcov.mjs` `ok=true`。
- 最新本地 V8/LCOV：statements `13284/18608 = 71.38%`；branches `7318/11610 = 63.03%`；functions `2585/3536 = 73.10%`；lines `11960/15895 = 75.24%`；距本地 statements 80% 尚缺 `1603` 条。
- `pnpm check`：`0 errors / 0 warnings`。
- 全量覆盖仍记录多个 `.mjs` `PARSE_ERROR/Expected ident`；未扩大排除范围、未以局部覆盖冒充 Sonar。

## 未闭环硬闸

- `REQ-SONAR-COVERAGE-80-01` 仍 `ACTIVE`：本机无 `sonar-scanner`，未设置 `SONAR_TOKEN`/`SONAR_HOST_URL`，故无真实 Sonar project coverage 与 Quality Gate 证据。
- PTY 五条件原子运行时快照、第三方 Runtime/A2A 真实兼容性、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限、移动 profile、Remote 四路径现场证据仍未闭环。
- 不能把本地 LCOV、受限文件扫描、NLM 建议或协议等价 E2E 记作上述现场/服务端验收。

## 工作区与接续规则

- 未向 Codex 之外 CLI、Agent 或 teammate 发消息。
- `coverage/*`、各包 `.iteration/*`、NotebookLM 子模块运行态及既有 E2E artifact 脏改动均保留、不纳入本轮提交。
- 当前 `sonar-project.properties` 有既有未提交修改，未擅自纳入；接续时须先确认其是否符合“全项目 Sonar”要求，禁止以排除 `scripts` 绕过 80% 闸。
- 接续顺序：先恢复可用 Sonar 服务端凭证并保留完整 source scope；再补真实扫描/Quality Gate；随后补 PTY 五条件与现场 E2E，最后重跑 intake、全量测试、`pnpm check`、Sonar 与交接归档。
