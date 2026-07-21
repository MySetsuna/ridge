# NotebookLM guidance — iteration 3

生成时间：2026-07-21 13:00 +08:00  
Notebook：`Ridge 项目现状、愿景与规划基线（2026-07-21）`  
Conversation：`a47d3199-c1f9-47f1-927c-ff2c4875b77d`

## NotebookLM 原始指导

NotebookLM 给出的 iteration 4 排序：

1. `[P0]` 移动浏览器真实生命周期 Smoke 与证据归档：iOS Safari、Android Chrome 执行 Wi-Fi/蜂窝切换和后台恢复；建议新增 Evidence Capture JSON 产品遥测。
2. `[P1]` Watchdog 升级与 ICE restart deadline 自动故障序列：建议新增 deadline 监控，验证僵死连接升级全量重建。
3. `[P1]` Not-ready 调用策略：新增 `RpcNotReadyError` 或短时排队；同时夹带只读 Agent roster。

唯一推荐方向是把 iteration 3 的实验室证据延伸到真实移动设备。NotebookLM 要求对 watchdog 阈值和 not-ready 队列 TTL/容量做独立对抗评审。

来源：iteration 3 报告 `b08a0025-6add-4eda-946e-4feeede4d0f3`、项目愿景/状态/差距基线及 iteration 2 报告。

## 对抗评审

独立 checker：`iteration_2_adversarial_review`；只读审核，未修改仓库。

| 候选 | 裁决 | 关键理由 |
| --- | --- | --- |
| 真机 iOS/Android smoke | 修改采纳，作为 iteration 4 唯一主目标 | 真实移动浏览器网络栈、后台冻结和 token 跨窗仍无物理证据；只做聚焦 runbook、人工执行与外置证据模板，不加产品遥测。 |
| 新增 watchdog/ICE deadline | 驳回实现，改为短自动前置 | `ControllerCloudProvider` 已有 15 秒 disconnected watchdog 和 12 秒 ICE restart deadline；缺口只是 iteration 3 门禁未覆盖“短抖动自愈”和“watchdog→ICE deadline→rebuild”两条升级时序。 |
| Not-ready 拒绝/排队 + Agent roster | 延期 | not-ready 期间 request 可能被 provider 静默丢弃并最终超时，确有后续 fail-fast 价值；但不应自动重放任意写请求。Agent roster 是无关扩项，远端 topology/capability 前置仍缺。 |

### 经检验后的 iteration 4 方向

唯一主目标：取得 iOS Safari 与 Android Chrome 在真实 Cloud Remote 换网、后台恢复场景下的可复核物理证据。

短自动前置：

1. `disconnected < 15s → connected`：不 ICE restart、不重建、不重复恢复。
2. `disconnected watchdog 15s → ICE restart → deadline 12s → rebuild`：旧资源关闭，新连接重新授权后只恢复一次，timer 清零。

文档/证据边界：新建聚焦 runbook；evidence JSON 只作仓外人工记录模板，含 commit/build、设备/OS/浏览器、网络切换、后台时长、恢复耗时、结果和脱敏附件路径。禁止 JWT、TOTP、账号和完整设备域名；不得为 smoke 新增 RPC、后端存储、产品遥测或身份字段。

人工门禁每个平台各一次：基线连接/TOTP/输入回显；Wi-Fi→蜂窝→Wi-Fi 无刷新恢复；后台跨过 15 分钟 token 窗口后恢复；pane、scrollback、输入和能力 UI 正常且无重复订阅。建议恢复目标 ≤45 秒，超出按失败记录，不放宽阈值掩盖。
