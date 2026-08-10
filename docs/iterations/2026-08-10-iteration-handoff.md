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
- `3d7981a6`：TerminalManager wasm 初始化/诊断、WebGPU 回退、主题/视口/DPR 与 raw-byte 拟合边界。
- 每波均含对应 `docs/iterations/2026-08-10-wave*-handoff.md` 与 `docs/REQUIREMENTS-SPEC.md` 证据；仅提交测试/文档，未改生产通信语义。

## 当前质量证据

- 聚焦回归：TerminalManager `26/26`、wsRemote `7/7`、fileEditor `13/13`、ridgeCloudProvider `19/19`、fileExplorer `46/46`。
- 全量 `pnpm test:coverage:sonar`：exit `0`，`normalize-lcov.mjs` `ok=true`。
- 最新本地 V8/LCOV：statements `13330/18608 = 71.63%`；branches `7366/11610 = 63.44%`；functions `2588/3536 = 73.19%`；lines `12003/15895 = 75.51%`；距本地 statements 80% 尚缺 `1557` 条。
- `pnpm check`：`0 errors / 0 warnings`。
- Rust 质量闸：`cargo fmt --all -- --check` 通过；`cargo test -p ridge-mcp --lib` 为 `90 passed / 0 failed`；`cargo test -p ridge-kernel --lib` 为 `49 passed / 0 failed`。仅见 Windows linker informational warning。
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

## Wave69 TerminalManager 生命周期与几何边界覆盖

- `packages/remote/src/shared/terminal/manager.attach.test.ts` 覆盖 wasm `ready()` 单例初始化、atlas/present-fast 诊断导出、WebGPU-first handle 成功路径及构造失败后的 Canvas2D 回退。
- `packages/remote/src/shared/terminal/manager.test.ts` 覆盖主题背景 RGBA 解析、active workspace 隐藏判断、共享 host/Canvas2D 视口投影、raw-byte 本地网格拟合、DPR 漂移重配置与 shared visual-only fit。
- 聚焦回归为 `26/26`，全量回归 `202` 文件、`1894 passed / 1 skipped`；`pnpm check` 为 `0 errors / 0 warnings`。
- 本波提交 `3d7981a6`，仅增测试，不改生产通信语义。
- 本波使本地 V8 statements 从 `13284/18608` 提升至 `13330/18608`；Sonar `>=80%`、Quality Gate、PTY 五条件、第三方 Runtime/A2A 与现场 E2E 仍按上文保持 `ACTIVE`，不以本地 LCOV 替代。

## Wave70 真实覆盖与适配边界（2026-08-10）

- 新增确定性测试：`ridgeTheme` SSR/浏览器回退、`contextMenu` 焦点/resize、`TauriDataProvider` 文件/Git/搜索映射、`hostReconnect` IPC/取消/loop、Remote SidebarProvider 全适配出口、TerminalManager pointer motion/自动滚动/cancel。
- 焦点回归：TerminalManager `27/27`、SidebarProvider `16/16`；全量 `pnpm test:coverage:sonar` exit `0`，`normalize-lcov.mjs` `ok=true`。
- 最新本地 V8/LCOV：statements `13625/18608 = 73.22%`、branches `7600/11610 = 65.46%`、functions `2677/3536 = 75.70%`、lines `12271/15895 = 77.20%`。本地 LCOV 仍不替代 Sonar 项目指标。
- 当前本地 SonarQube `26.7.0.124771` 与 scanner 均可用；产品源边界扫描（`src,packages,src-tauri/src`，沿用既有配置排除测试/构建物）scanner/CE 均 `SUCCESS`，CE task `bf37aa7a-d256-44a1-90e1-65bab8949092`，项目 coverage `80.2%`、line `86.5%`、branch `71.3%`，但 Quality Gate `ERROR`：`new_coverage=86.8% OK`、`new_duplicated_lines_density=1.06625% OK`、`new_violations=126 ERROR`；项目 violations `672`、bugs `15`、vulnerabilities `14`、code smells `643`。
- 同日完整源集（额外纳入 `scripts`，不以排除脚本冒充全项目）扫描 scanner/CE 亦成功，但项目 coverage `66.3%`、line `67.5%`、branch `64.3%`，Quality Gate `ERROR`（`new_coverage=17.8%`、`new_violations=155`）。故 `REQ-SONAR-COVERAGE-80-01` 仍 `ACTIVE`：产品源口径已过 80%，全源口径与 Quality Gate 尚未闭环。
- 新问题审计：当前 new-period 查询 `172` 条，主要规则为 Rust `S3776/S107`、TypeScript `S2933/S3735` 等复杂度/类型安全问题；本波未通过改 Quality Gate、排除未测代码或静态估算绕过，需下一轮逐项修复并重扫。
- 扫描日志：`.tools/sonar-scan-codex-20260810-wave70-product.log`、`.tools/sonar-scan-codex-20260810-wave70.log`；临时副本仅用于规避 Windows Rust CRLF scanner 偏移，未改工作树源码。凭证为一次性本地 token，未写入仓库。

## Wave71：全源脚本质量与 Sonar 复扫（2026-08-10）

- 将 executable verification harness 与 product helper 分开归类：`sonar.sources` 保留 `scripts`，真实 `src/packages` 测试继续由 `sonar.tests` 管理，测试文件由 `**/*.test.*` / `**/*.spec.*` 排除；脚本 harness 保留在 source scan 以接受静态质量检查，未通过排除 `scripts` 获得覆盖率。
- 对 build/asset/dev/signaling/review/publish helper 增加可注入函数边界与 main guard；新增 `scripts/quality-helpers.test.mjs`，聚焦回归 `25 passed / 0 failed`。本轮额外修复 Sonar 报告的脚本认知复杂度、promise-chain、正则回溯、嵌套模板、默认对象参数及回滚/manifest 分支问题。
- 最新全源 Sonar：scanner/CE `SUCCESS`，CE task `3c78dac8-13b1-41a8-a65e-cdce622ee4cd`；`coverage=80.2%`、`line_coverage=86.4%`、`branch_coverage=71.6%`、`lines_to_cover=15236`、`uncovered_lines=2074`。Quality Gate `ERROR`：`new_coverage=81.7% OK`、`new_duplicated_lines_density=0.94426% OK`、`new_violations=152 ERROR`；项目总计 `violations=730`、`bugs=17`、`vulnerabilities=21`、`code_smells=692`。故仅覆盖率目标已达，质量门未达，不宣称完成。
- `new_violations=152` 的主因仍为 Rust `S3776=61`、Rust `S107=13`，其次为脚本/TS `S2933/S3735/S5906/S6582/S3358` 等；修复须逐项重构/断言，不以 `NOSONAR`、排除源集或改 Gate 绕过。
- Agent 通信架构审计结论：Hub 的 PTY 五条件、TTL、generation/lease fencing 与确定性测试存在；桌面 `DesktopMcpHost::probe_delivery` 生产路径只有 MCP pull 与已注册 Runtime/A2A probe，尚无一个同时读取 `agent_idle`、`terminal.mode=agent_prompt`、`pending_approval`、前台目标进程、用户输入竞争并以 revision/epoch 发布的运行时采样器，故 PTY 仍 fail-closed。第三方 CLI 私有 Runtime/A2A 实测未进行，亦未宣称兼容；本轮不向 Codex 外 CLI/Agent 发消息。
- 本轮 `pnpm check`：`0 errors / 0 warnings`。既有 `coverage/*`、`.iteration/*`、E2E artifact 及 `packages/remote/src/shared/cloud/__faultRig.ts`、`faultInjection.test.ts` 脏改动均不纳入本轮提交。

## Wave72 最终回归（2026-08-10）

- 质量提交 `08d8c1c6`：remote/cloud、pane/terminal、workspace 质量路径与随机回退护栏；新增 `paneFeedScheduler` 调度/dispose 分支测试。
- `pnpm test`：`207` files，`1943 passed / 1 skipped`；`pnpm check`：`0 errors / 0 warnings`；`pnpm build`：exit `0`，Vite `4222 modules`。
- CodeGraph 复核未见 CloudHostPaneSource、ControllerCloudProvider、PaneRpcScheduler、TerminalManager geometry、workspaceScope 或构建门禁断链；外部 Tauri WebView2 重连、真机 PWA/IME、原生 DPR、跨卷 ACL 中窗、Sonar Gate 仍为开放外部证据项。
