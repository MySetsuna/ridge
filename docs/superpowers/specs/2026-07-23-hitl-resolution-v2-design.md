# Remote HITL 裁决通道设计（P2 阶段 2，iteration 11 G2——仅设计，零代码）

日期：2026-07-23。前提：阶段 1（iteration 8）已落——远端脱敏只读列表 `list_hitl_pending`；裁决仍仅桌面（`resolve_hitl_request` 不可远达）。本文设计**远端裁决**语义；**实现被红线冻结**（用户轨证据消化前不扩协议面），设计先行以缩短未来实现轮。

## 1. 威胁模型与目标

远端裁决 = 高危命令的放行权离开桌面信任边界。攻击面：①重放（截获旧「approve」重打）；②竞态双消费（两 controller 同时裁决 / 同 controller 重试）；③会话残留（断线重连后旧裁决落到新挂起项）；④审计缺失（谁批的、批了什么无从追溯）。目标：单条裁决流在 E2EE 信道内做到**一次性、可追溯、fail-closed**。

## 2. 核心机制：服务端一次性裁决票据（方案 A，选定）

- host 在 `list_hitl_pending` 投影中为每个挂起项附 `resolutionNonce`（随机 16B base64，随挂起项生成、存 `PendingEntry`）。
- 远端裁决请求：`resolve_hitl_remote {id, nonce, verdict}`（verdict ∈ approve/reject；**modify 排除**——远端注入替换命令 = 远程任意命令执行面，永不开放）。
- host 校验：id 存在 ∧ nonce 恒时比对一致 → **取出即毁**（沿既有 `PENDING.remove` 单次消费语义，天然防双消费竞态：第二个裁决落空返回 `already-resolved`）；不一致 → 拒并计数（S1 模式）。
- 对比方案 B（挑战-响应握手）：多一轮往返、需临时状态机；单次票据在 E2EE + TOTP 已授权信道内强度足够（nonce 只经加密信道可见，截获前提是信道已破）。**选 A**。

## 3. 过期与 fail-closed 竞合

- 既有 120s 超时 fail-closed 拒绝**不变不延**（远端裁决不改本机安全时钟）。
- 超时后到达的远端裁决：`PENDING` 已空 → 返回 `already-resolved`，**不**产生副作用。
- nonce 随挂起项同生共死，无独立过期表（YAGNI）。

## 4. 多 controller 冲突

- **首达生效**（single-consume 自然裁定）；后到者收 `already-resolved`。
- 审计记录**含败者尝试**（见 §5）：两 controller 同时批/拒时，人可事后看到冲突发生。

## 5. 审计（接 M1 `decisions` 切片二）

每次裁决（含本机、远端、超时、落空尝试）追加 `decisions` 条目：`{ts, source: desktop|remote(cid 前 8 位)|timeout, verdict, riskLevel, reasonSummary, outcome: consumed|already-resolved|nonce-mismatch}`。**绝不存命令全文**（沿阶段 1 投影纪律）；落盘走 M1 sidecar 管道。

## 6. 传输面选型

| 候选 | 评估 |
| --- | --- |
| A. `teammate` 能力下新只读+新写方法（`resolve_hitl_remote` 入 `REMOTE_ALLOWLIST` + `MUTATING_METHODS`） | 与 `list_hitl_pending` 同面对称；走既有 A2 宣告纪律 + 合同测试；LAN/cloud 双入口自然覆盖 |
| B. 0x12 CONTROL 通道扩帧 | CONTROL 面语义是**连接级验证**（totp/trust），塞业务裁决混层；LAN 入口无此通道，需另做一套 |

**选 A**。宣告纪律：六处同步 + 矩阵 + `MUTATING_METHODS` 归类 + 负断言更新（`resolve_hitl_request` 桌面版仍不可远达；远端版为独立方法，语义收窄无 modify）。

## 7. 桌面/远端一致性

- 桌面 modal 裁决与远端裁决共享同一 `resolve` 内核与单次消费语义；桌面版保留 modify（本机信任边界内）。
- UI：Team 面板 Pending 区（阶段 1 已有）加 Approve/Reject 双按钮 + `already-resolved` 反馈；无确认对话框之外的第二次确认（nonce 即防误触的技术面，UX 层由按钮距离与颜色承担）。

## 8. 实现轮验收路径（合同素材）

- Rust：nonce 恒时比对、单次消费（并发两裁决恰一成功）、超时后裁决落空无副作用、审计条目无命令全文——各一测。
- TS：合同测试 `resolve_hitl_remote` ∈ allowlist ∧ ∈ MUTATING_METHODS ∧ modify 不可达；UI 双按钮 + 反馈。
- 全门禁绿 + 真机人工核验一次远端裁决闭环（用户轨）。

## 9. 明确不做

审批委托、批量裁决、自动过期策略配置、远端 modify、审批规则引擎——皆边缘逻辑，待真实使用证据。
