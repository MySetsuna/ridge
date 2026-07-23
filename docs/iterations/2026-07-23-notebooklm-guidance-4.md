# NotebookLM guidance — iteration 4 收口 → iteration 5 规划

生成时间：2026-07-23 11:45 +08:00
Notebook：`Ridge 项目现状、愿景与规划基线（2026-07-21）`（已压缩至唯一来源 `PROJECT-STATE`，source id `41de50ce`）
Conversation：`a47d3199-c1f9-47f1-927c-ff2c4875b77d`

## NotebookLM 原始指导（摘要）

1. **主线裁决：选 S1**（安全回落构造点矩阵 + 遥测/退役设计），理由排序：风险降低（CloudHostBridge 未注入 verifier 时默认放行）> 解锁力（S1 是 fail-closed 与 P2 HITL 的前置）> 可逆性（审计动作高可逆）> 用户价值 > 成本（低于 P1）。
2. 目标清单：S1 主线；T3 生产状态只读脚本（称可「通过 Dokku API 获取 SHA、curl 探测 artifact 指针」）；T1 `cargo test --workspace` 绿灯；A2 机器可读矩阵 `docs/capability-matrix.json` + 一致性校验；A1 按调用图删 3–5 个冗余 handler。
3. 减法：R1 真机证据归档前 Remote 侧不加新写路径；E1 WebGPU 挂起。
4. 里程碑地图：M1 可信基线固化（T1/T3/S1）→ M2 控制平面可见化（P1/R1/A2）→ M3 交互可控与对齐（P2/A1/C1）→ M4 深度连续性与协作（G1/M1/M2/H1）。

## 对抗评审（checker：本地 codegraph + 源码 + 实测）

| 候选 | 裁决 | 关键证据 |
| --- | --- | --- |
| 主线选 S1 而非 P1 | **采纳** | `cloudHostBridge.ts` 注释与代码确认「verifier 未注入则兼容放行」真实存在；S1 先审计不改行为，可逆性论证成立。 |
| S1 验收「关键路径 100% 覆盖遥测断点」 | **驳回措辞，改写验收** | 「100% 遥测断点」不可被编译器/测试/退出码判定，且本轮不实现产品遥测（沿用 iteration 3 熔断）。改为：①构造点矩阵文档（枚举 desktop host / CLI host / LAN / controller 各入口 × 各 verifier 的注入与回落状态，含 `file:line` 证据）；②确定性测试：断言 `CloudHostBridge` 在 verifier 注入/缺失两种构造下的门控行为与回落面（exit 0 可判）；③遥测与退役仅出设计文档。 |
| T3「Dokku API 获取 SHA + curl 探测 artifact 指针」 | **驳回事实，改写方案** | grep `ridge-cloud/src/router.rs`：artifact 仅有 `POST /remote-artifacts`（上传）与 `POST /remote-artifacts/rollback`，**无任何 GET 状态端点**；`GET /api/v1/health` 只返 `{version, uptimeSecs}` 无 git SHA；Dokku 无默认 HTTP API（SSH 语义）。改为：在 ridge-cloud 新增 token 守卫的只读 `GET /api/v1/remote-artifacts/status`（current 版本 + manifest gitSha/builtAt + 保留 releases），配 cargo 测试；wind 侧一键脚本汇总两条版本线证据；对生产实跑由用户执行（凭据/网络在用户手）。 |
| T1 cargo 全仓绿灯本轮补齐 | **采纳（实测中）** | 根 `Cargo.toml` 为 8 成员虚拟 workspace；`cargo test --workspace` 已在本机启动，结果记入合同执行。 |
| A2 机器可读矩阵 | **采纳，锚点已核** | canonical 锚点存在：`ridge-core/src/capability.rs::{CapabilitySet,REMOTE_ALLOWLIST}` 与 `remote/src/shared/transport/capabilityContract.ts::REMOTE_CAPABILITY_METHODS`；矩阵须由测试与 canonical 声明互证，不做第二事实源。 |
| A1「删 3–5 个冗余 handler」 | **收窄** | GAP_PORTFOLIO 原则为「每次只删除一个有等价共享路径 + 回归覆盖的重复面」；「3–5 个」为无证据的数字目标。收窄为：审计报告必做；至多一个满足条件的示范删除，无则只出报告。 |
| 减法条款（Remote 无新写路径、E1 挂起） | **采纳** | 与愿景决策过滤器一致。 |
| 里程碑地图 M1–M4 | **采纳为方向** | 与 GAP §7 近期组合一致；不作为逐字承诺。 |

## 经检验后的 iteration 5 方向

见 `CONTRACT-iteration-5.md`：主线 S1（审计 + 门禁测试 + 设计文档），支线 T3（ridge-cloud 只读 status 端点 + 汇总脚本）、T1（双仓 cargo 绿灯证据）、A2（机器可读矩阵 + 一致性测试）、A1（收窄审计）；真机 smoke（R1）保持用户必办件，不入本合同目标。
