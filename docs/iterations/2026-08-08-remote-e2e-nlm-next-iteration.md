# Remote / pane / workspace / NLM 迭代收口（2026-08-08）

## 范围与授权边界

本轮完成本地代码审计、修复、测试、Sonar 扫描与 dev:cdp E2E。未执行 GitHub Release、Remote/Cloud 上传、生产激活或任何需要外部发布授权的动作；发布只能由用户明确授权。

## NLM 近期痛点并入本轮

已重新认证 `nlm`，`nlm login --check` 与笔记列表均通过。主笔记：`Ridge 项目现状、愿景与规划基线（2026-07-21）`。本轮只读抽取近期对话痛点，未删除或上传 NotebookLM source：

- kernel-first 与 Tauri shell 解耦后，桌面 pane tree、kernel topology、Remote projection 易漂移。
- pane 身份必须使用 `(workspaceId, paneId)`，跨 workspace、跨窗口、PTY bridge 与订阅不可只用 `paneId`。
- Remote 重试、移动端连续性、geometry/DPR、PTY 输出与渲染稳定性需同一条可验证链路。
- detached kernel/Remote 连接需有 readiness、重连、拓扑同步及资源清理边界。

## 本轮落地

- Remote host 状态以实际 host/sidecar 为准；修复 stale readiness/cache 导致的假启用与接入失败。
- 桌面 workspace 切换、窗口切换、pane split/close、PTY 安装均同步 kernel topology；detached kernel 轮询拓扑变化并推送布局/错误信息。
- PTY 订阅补充 bounded OSC metadata 解析，发送 `pty-meta` 的 title/cwd；清理 dirty cwd 解析路径。
- pane close 先拆 PTY bridge，再关后端 pane；失败时按 workspace 恢复，避免 close 事件触发错误重建。
- 修正移动端触摸选择模式的不可达分支。
- dev:cdp 每次构建使用唯一 dev `rdg` sidecar，避免复用 stale binary。
- Sonar 新增问题涉及的 Error 类型、Promise 初始化、重复赋值与 Rust 订阅复杂度已收敛。

## CodeGraph 复核

已复核 `sync_kernel_workspace_topology`、`sync_domain_workspace_topology`、`switch_workspace`、`switch_window_workspace`、`insert_new_workspace`、`split_pane`、`close_pane`、`closePane`、`teardownPtyBridge`、`ensurePtyBridge`、`start_subscription`、`send_subscription_data` 与 `handleTouchStart` 的调用关系。当前审计链覆盖 desktop → kernel → Remote topology，以及 composite pane ref → PTY bridge → subscription；未发现 reviewed path 仍以单独 `paneId` 作为跨 workspace 身份的遗漏。

## 验证结果

| 检查 | 结果 |
|---|---|
| Vitest 全量 | 155 files；1606 passed，1 skipped |
| `pnpm check` | 0 errors，0 warnings |
| `ridge` workspace focused Rust test | 1 passed |
| `ridge-cli` kernel host focused Rust tests | 8 passed |
| `ridge-kernel` domain focused Rust tests | 20 passed |
| coverage | 生成 `coverage/lcov.info`；本轮 paneTree 统计 statements 37.22% / branches 25.98% |
| SonarScanner | 上传成功；Quality Gate `OK`；new violations 0；new duplicated lines 0 |
| pane graph E2E | split/close、LAN pane frames 通过 |
| PTY parser E2E | binary、metadata、title/cwd 通过 |
| terminal input E2E | injected 通过 |
| LAN probe E2E | hello/panes/subscribe/scrollback/live echo/pong/UUID 全通过 |
| teammate E2E | 7/7 通过 |
| `rdg-remote-e2e` | desktop/mobile ALL PASS；desktop 脚本报告 `tree=false`，canvas/ws 仍通过 |
| mobile keyboard E2E | visual/input/shift/recovery/selectionTouch/copy 全通过 |

Sonar 仅代表本次 new-code gate：服务器报告 `ignoredConditions: true`，`new_coverage` 实测 34.8，故不可表述为整体覆盖率达标。历史整体 baseline 仍有既存 bugs、vulnerabilities、code smells 与低覆盖率；扫描日志另有 Rust highlighting 及 `paneTree.ts` 编码告警。

## 下一迭代已登记问题

1. `BUG-REMOTE-E2E-001`：`cdp-multitab-freeze` gate 虽通过，但 console 收到两次 `create_pane (rebuild) failed`。下轮需采集每次失败的 workspace/pane、后端错误与重建前后 topology，并补确定性回归测。
2. `BUG-REMOTE-E2E-002`：detached/headless LAN probe 未收到 theme frame（`theme=false`）。下轮补 kernel theme projection 或明确协议降级，并加契约测。
3. `BUG-REMOTE-E2E-003`：dev 日志仍出现 orphaned kernel PTYs 增长及 `resize_pane_missing` / `create_skip_exists`。下轮建立 bounded reconciliation/cleanup，验证超时、取消、进程树回收与计数归零；不得直接杀 installed kernel/daemon。
4. `BUG-REMOTE-E2E-004`：移动端验证为 Chromium mobile emulation，尚非实体手机/PWA soak；下轮需用户设备或明确环境授权。
5. `BUG-REMOTE-E2E-005`：`rdg-remote-e2e` 约 90 秒；下轮优化 runner 超时、清理与失败证据留存。

## NLM 下一轮工作流

下一轮以本文件、`docs/PROJECT-STATE.md` 与现有主笔记为输入，先做 NLM 只读对抗提问：逐项挑战上述五个 bug 的根因、验收条件、遗漏调用路径及发布风险；再生成下一轮 contract/decision，最后才改代码。未获用户授权前，不删除/上传 NotebookLM source，不发布 Remote/Cloud，不激活生产。

证据：`.iteration/artifacts/rdg-remote-e2e/last-result.json`、`.iteration/artifacts/rdg-remote-e2e/mobile-keyboard.json`、`.tools/sonar-scan-final-current.log`。
## 本轮最终校正（2026-08-08）

- NLM 已重新认证：`nlm login --check`、笔记列表（22 本）及主笔记对话仅读抽取均成功；未删除、上传或修改 NotebookLM source/note。并行审查 pane 未执行，因 Claude pane 未登录，未将其输出冒充证据。
- NLM 痛点已映射：kernel-first/双 SSOT、`(workspaceId,paneId)` 复合身份、Remote readiness/reconnect/reap、theme 帧、multitab freeze、移动 geometry/DPR 与 PTY/render 连续性。
- 已修：KernelHost legacy workspace/pane result、空 workspace 首 pane、tree reconnect 偏好、两类 Host 共享 theme wire frame、kernel bootstrap 后全 workspace topology sync、remote 脚本动态 CDP/端口与重建竞态降噪。
- BUG-001 最终处理：`pane-pty-closed` 重建若 pane 已合法销毁，`create_pane` 的 `Pane not found` 视为 late-close race；非该错误仍记 error。新增 ptyBridge 回归测，18/18 通过；multitab 冷启动后 1→2→3 workspace 无重建错误。
- E2E：LAN theme/hello/panes/UUID/echo/pong、pane split/close broadcast、PTY parser、multitab、teammate 7/7、`rdg-remote-e2e` desktop/mobile、mobile keyboard 均有通过证据；移动结果仍仅 Chromium emulation，非物理设备。
- 质量：Vitest 155 files / 1610 passed / 1 skipped；`pnpm check` 0/0；KernelHost 12/12；ridge-core theme 18/18；`cargo fmt --check` 通过。受限 4 文件 Sonar 扫描质量门 `OK`（new coverage 80.0、new duplication 1.05125、new violations 0）；全项目 TS analyzer 仍有超时，已留日志。
- 发布闸：未发布、未上传 Remote/Cloud、未激活生产；任何发布只能由用户明确授权。

## 最终复跑与质量边界（2026-08-08 23:00）

- CodeGraph `sync` 已完成；随后复核 composite PTY bridge、shared theme frame、KernelHost、workspace topology sync。
- 最终 live：LAN `RESULT: PASS`（theme/hello/panes/subscribe/scrollback/live echo/pong/UUID）；pane graph split/close broadcast PASS；multitab 1→2→3，最大 event-loop lag 46ms、long task 0；PTY parser、term input、teammate 7/7、`rdg-remote-e2e` desktop/mobile `ALL PASS`。
- 最终静态：Vitest 155 files / 1610 passed / 1 skipped；`pnpm check` 0 errors / 0 warnings；`cargo fmt --check`；ridge-core theme 1/1；ridge-cli KernelHost 12/12。
- Sonar 重扫因本地服务器返回 HTTP 401 未取得新结果；既有受限四文件 Quality Gate `OK` 仅代表此前快照，不能覆盖本轮 ptyBridge 最终变更，也不能表述为全项目质量门通过。监控页面亦因当前无可用 App Browser 未打开。
- multitab 仍偶发一次 `Explorer loadTree: missing path C:/code/wind/src-tauri` 非致命 warning；直接 `get_file_tree` 与分页 host probe 随后成功。已登记下一轮做 transient missing-path 重试/证据化，当前不掩盖该异常。
- 资源回收补验：`cdp-reap-test` PASS；带 `CDP_PORT=10486` 的 `remote-leak-trace` 完成 pane/workspace/reconnect/reap 全流程，`reap pass1=0`、`pass2=0`，未复现 orphan/re-creation cycle。

## NLM 对话回流校正（2026-08-08）

近期 chat 另暴露工作流层痛点：模型调查过久后 `max stall` 强退且解释不透明；历史输出不能自然滚动审计；输入框提示挤占视口；搜索路由、Jules 隔离与 Sonar/Coverage 遥测容易被 NLM 叙述成“已实现”。这些均已登记为下一轮待证伪项，不能以 NLM transcript 代替本地验收。

本轮修补 `notebooklm-iteration-loop`：NLM 默认只读；近期 chat 只落本地痛点归档；NLM 的“批准/完成”声明不产生需求授权；来源/笔记写操作、发布、Remote/Cloud 激活与 `git push` 均要求用户当前对话逐项授权。主流程继续遵守 CodeGraph/源码/测试优先。
