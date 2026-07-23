# S1 回落遥测与退役设计（设计先行，不实现）

日期：2026-07-23 · 状态：设计稿，待评审
前置：`docs/security/cloud-fallback-matrix.md`（构造点矩阵与回落面 F1–F6）

## 目标

在不改任何回落行为的前提下，让每个回落面的**真实发生率**可观测，据数据决定各 fail-closed 翻闸时机。
原则：遥测是退役的前置，不是常驻产品功能；每个回落面配「退役条件 + 到期动作」。

## 非目标

- 不做用户级分析、不含账号/设备明文标识、不上报终端内容。
- 不新增后端存储表；沿用现有日志/指标通道。
- 不在本设计内翻任何 fail-closed 开关。

## 采集点设计（最小集）

| 事件 | 采集侧 | 字段（全部低基数枚举/计数） |
| --- | --- | --- |
| `handshake_mode` | host（桌面/CLI）+ controller | `frame: 0x01\|0x02`、`binding: enforced\|relay-trust`、`entry: desktop\|cli\|spa-desktop\|spa-mobile` |
| `totp_path` | host bridge | `path: plain\|bind\|trust-skip`、`ok: bool`、`locked: bool` |
| `trust_proof_transcript` | host bridge | `transcript_present: bool`（F1 关键信号） |
| `tofu_change` | controller | `action: warned`（F3；仅计数） |
| `grace_fallback` | 双侧 | `reason: no-signaling-pubkey`（F2；仅计数） |

传输形态：host 侧走现有 tracing/log 计数（本地聚合，人工采集窗口读取）；SPA 侧仅本地
console/诊断面板计数，不回传服务器。**第一阶段人工读数即可**——样本量小，无需自动化管道。

## 退役门（每面一条可判定规则）

- F1：连续 30 天 `trust_proof_transcript{present:false}` = 0 → host 要求 transcript 必在（改桥 + 测试翻转）。
- F2：`binding:relay-trust` 占比 < 1% 且已知旧端全部升级 → 宽限回落改拒绝。
- F3：「确认新指纹」UI 落地 → TOFU 变化默认拒绝待确认。
- F4：现存设备身份密钥覆盖率 100%（`get_device_identity_pub` 不再失败）→ cloud 入口拒 0x01。
- F5：确认 §5.5 钩子无启用计划 → 直接删除钩子与相关分支（净减法）。
- F6：构造器改为必传其一校验器（类型层面强制），删「双未注入=不门控」分支。

## 验收信号（实施轮用）

- 每个采集点有单测断言事件在对应路径恰好计数一次。
- 退役翻闸各自独立 PR：翻转既有 S1 门禁测试期望值即为行为变更的确定性证明。

## 风险与回滚

- 遥测本身零行为影响；翻闸每面独立、可单独回滚。
- F2/F4 翻闸前必须公告旧客户端升级窗口（rdg CLI 与已安装桌面版）。
