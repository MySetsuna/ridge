# Ridge 迭代日志

按时间倒序追加；每条记录包含已完成事项、下一步、熔断与被驳回建议。

## 2026-07-23 — iteration 6

- 完成：P1 控制台 MVP 代码侧关闭——新协商能力 `teammate`（唯一只读方法 `get_teammate_topology`）六处同步宣告 + LAN/cloud 路由 + 共享 controller Team roster 面板（轮询/切 pane/无团队零噪声）；投影脱敏 Rust 测试与 HITL 不可远达负断言。S1 遥测第一阶段（F1/F2 进程内计数 + 测例钉死）。A1 切片：workspace 列表投影同源化（删平行 WorkspaceInfo，net −12）。
- 验证：vitest shared 全伞 558/558（1 skipped）、svelte-check 71 files 0 errors、cargo workspace-excl-ridge 882 绿、ridge bins 27 绿、`--lib --no-run` 可编译。
- 熔断：无停机触发；cloudHostBridge 硬编码七能力清单按合同更新为八。
- 下一步：iteration 7 证据与固化轮（冻结新功能）：主线修复 `-p ridge --lib` loader 载败；弱网实验室参数化 harness；Teammate 字段审计；用户必办件清单单页。
- 驳回：NotebookLM 的「TURN-only/真实丢包自动化验收」（需真实网络工装/凭据，改双轨：自动轨实验室 harness 显式标注，不冒充真机结论）；T3 实跑与真机归档入自动合同（凭据/设备在用户手，列用户轨）；「清理 Teammate 冗余字段」直接删（收窄为审计先行）。采纳：冻结轮、loader 修复升格、`list_hitl_pending` 方案归档待 P2。

## 2026-07-23 — iteration 5

- 完成：可信基线固化五目标全落——S1 构造点矩阵 + 3 条回落钉死测试（发现并更正「无 transcript 时 trust-proof 直接失败」的错误注释：实为退化为无信道绑定 + 信任库裁决）+ 遥测/退役设计；T3 ridge-cloud 只读 status 端点（124 绿，独立分支）+ wind 一键两线脚本（桩验四径）；T1 双仓 cargo 绿灯（wind 882+27，cloud 124）；A2 机器可读矩阵 + 6 条一致性测试；A1 审计报告 + 删死 pane-output 面（−45 行）。
- 计划外：根治 signaling drift 门禁 Windows EOL 误报（.gitattributes 钉 LF）；vitest cloud+transport 全伞 382 绿。
- 熔断：`cargo test -p ridge --lib` 宿主 `STATUS_ENTRYPOINT_NOT_FOUND` 载败（loader 级、先于本轮、其余全绿）——如实记录不宣称全仓绿，根因挂开放问题。
- 下一步：iteration 6 主线 P1 Remote Agent 控制台 MVP（capability `teammate`、只读 `get_teammate_topology`、轮询、rdgHost denied）+ S1 计数实施 + A1 只读三件套同源化。
- 驳回：NotebookLM 的 `agent_roster_view` 命名（改 `teammate` 对齐短名词惯例）、`subscribe_agent_updates` 订阅流（MVP 轮询即可）、rdg-Host Supported（无 roster 数据源，denied）、`telemetry.log` 新持久面（按设计用进程内计数）、T3 挂 CI（无测试 CI，属用户决策）、移除 WebGPU 构建检查（违反 E1「先证伪再删」）。

## 2026-07-23 — iteration 4 收口 + 笔记本压缩

- 完成：补跑 iteration 4 自动验收全绿（faultInjection 7/7 含两条新 watchdog 升级时序、Cloud 定向 156/156、增量 svelte-check 0 errors、evidence 校验脚本自测 exit 0）；写收口报告。真机双平台 smoke 证据仍空，保持唯一用户必办件（runbook/schema/校验脚本已备）。
- 笔记本压缩：三份基线文档 + checker 附录导出归档至 `docs/ridge-baseline-digest/`；骨架合并进新唯一状态文档 `docs/PROJECT-STATE.md`；上传确认后删除全部 7 份旧来源，终态 1 来源（经用户「轻量多余来源」指示授权）。
- 下一步：iteration 5 可信基线固化——S1 构造点矩阵 + 门禁测试主线；支线 T3 只读 status 端点与汇总脚本、T1 双仓 cargo 绿灯、A2 矩阵机器可读化、A1 收窄审计。
- 驳回：NotebookLM 的 S1 验收「100% 遥测断点」（不可判定且本轮不实现遥测）；T3「Dokku API + curl 探测 artifact 指针」（grep 证实无 GET 状态端点、health 无 SHA，改为新增只读端点 + 脚本）；A1「删 3–5 个 handler」数字目标（收窄为审计 + 至多一个示范删除）。

## 2026-07-21 — iteration 3

- 完成：建立 Controller provider→adapter→RpcClient 与 Host 背压的确定性 fault-injection 门禁；100 周期验证无 pending RPC、重复恢复或 timer 泄漏。
- 修复：L2 原先在 E2EE connected 但 TOTP 未授权时提前发送 hello/pane recovery，Host 丢弃后不补发；现以 connected + authorized 为 business-ready。
- 验证：修复前新门禁稳定红灯；修复后 fault 5/5、Cloud 定向回归 161/161、增量 svelte-check 70 files/0 errors、Remote build 均 exit 0。
- 下一步：iteration 4 做 iOS Safari/Android Chrome 真机换网与后台生命周期 smoke；短前置补 watchdog/deadline 两条升级时序和外置证据模板。
- 驳回：不新增已存在的 watchdog/deadline；不为 smoke 加产品遥测；Not-ready 排队和 Agent roster 不夹带进入本轮。

## 2026-07-21 — iteration 2

- 完成：建立 Controller-facing 最小 capability→RPC 合同与跨入口测试；补齐 rdg `get_file_tree/read_file/text_search` 实际路由；Remote Files/Git/Search、workspace 管理与 theme UI 按 D9 能力协商隐藏/收敛。
- 验证：修复前门禁稳定命中 `get_file_tree` 缺路由；修复后 Remote 定向 Vitest 47/47、ridge-cli rpc 16/16、session 10/10、ridge-core capability 10/10、dispatch 25/25、Remote production build 均 exit 0；增量 svelte-check 70 files、0 errors、exit 0。
- 下一步：iteration 3 仅做 Cloud Remote 重连/背压/恢复的确定性跨层 fault-injection 门禁，优先只改测试。
- 熔断：默认 `pnpm check` 300 秒零诊断超时；checker 证明 `--incremental` 24.6 秒可完成，故未误升为工具链 P0。全仓 rustfmt 被既有格式债务阻断；未批量改无关文件。
- 驳回：NotebookLM 的 `pnpm check --filter ./packages/*` 命令无效；随机 5% 丢包 100% 和 NLC 不作为 CI 验收；Agent 控制台仍缺 Remote topology/安全合同与真实 pane-switch API，继续延期。

## 2026-07-21 — iteration 1

- 完成：校准《远程终端迭代规划与质量修复指南》的陈旧项；修复 Cloud TS capability mirror 缺 `get_workspace_snapshot`、mutating mirror 少 11 项；将固定计数测试改为 Rust canonical 逐项 parity。
- 验证：Remote 定向 Vitest 48/48、transport conformance 32/32、`ridge-term` clear 定向测试、WASM dev build、Remote production bundle 均 exit 0。
- 下一步：iteration 2 仅做“能力宣告→allowlist→handler→UI”的跨入口合同门禁。
- 熔断：note 五类原始建议大多已实现，触发来源陈旧熔断；`pnpm check` 可启动但 180 秒无诊断后超时，完整门禁未宣称全绿。
- 驳回：NotebookLM 的“恢复工具链为 P0”已过期；Remote Agent 控制台在数据能力远程化前不启动；弱网与真机风险无数据，不做确定排序。
- 后续：用户授权跨仓修改后，已同步最新 ridge-cloud，并将 wind 陈旧协议全文收敛为 canonical 入口 + 自动守卫；协议双 SSOT 债务关闭。
