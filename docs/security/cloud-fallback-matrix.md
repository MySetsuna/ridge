# Cloud Remote 安全回落构造点矩阵（S1 审计）

审计日期：2026-07-23（分支 `codex/remote-git-diff-iteration-1`）
方法：逐构造点读源 + 确定性门禁测试钉死回落语义（`cloudHostBridge.test.ts` §"S1 兼容回落面"）。
结论先行：**所有生产构造点均已注入 TOTP 类校验器**（无「完全不门控」的生产路径）；回落面集中在
①身份绑定宽限期 relay-trust、②TOFU 指纹变化仅告警、③trust-grant 无 transcript 时退化为无信道绑定、
④`keyBindingVerifier` 钩子全生产未接。

## 1. 构造点 × 校验器矩阵

| 构造点 | 明文 TOTP | totp-bind（信道绑定） | trusted-controller | 设备身份 0x02 | E2EE pubkey 绑定（B3） |
| --- | --- | --- | --- | --- | --- |
| **桌面 cloud host**<br>`src/lib/remote/cloud/cloudHostStore.ts:77-97` | ✅ `verify_remote_totp`（:85） | ✅ 仅当 bindTranscript 非空（:88-94）；host 未发 0x02 → undefined = 仅明文路径 | ✅ bindTranscript 注入（:96），trust 库经 `totp_trust_check/record` | ✅ 签名侧：`sign_device_identity` + identityPub（:54-58）；**取不到身份密钥 → 回落 0x01**（:110-117） | provider 侧 B3（见下行） |
| **桌面/CLI 共用 host provider**<br>`packages/remote/src/shared/cloud/ridgeCloudProvider.ts` | —（由 bridge 承担） | — | — | 发送侧 0x02/0x01 | ✅ `decideKeyBinding` 每 controller（:520）；**信令公钥 3s 宽限未到 → relay-trust**（:48-49,:531）；`verifyPeerKey` 钩子默认 relay-trust（:557） |
| **CLI cloud host**<br>`packages/ridge-cli/src/session.rs` | ✅ `RemoteTotp::load_or_create` 恒建（:173），verify（:587） | ✅ 仅当 bind_transcript（0x02 后）非空（:599-601）；**None → totp-bind 一律失败**（:568-569） | —（CLI host 无 trust 库；无此通道） | ✅ `encode_signed_frame`（:35） | ✅ `decide_key_binding`（:218）；**宽限过期 → relay-trust**（key_binding.rs:75-79） |
| **LAN host**<br>`packages/ridge-remote/server_app.rs` + `auth.rs` | ✅ `RemoteAuth`（委托 ridge-core `RemoteTotp` 唯一权威）+ session token（Bearer/`?token=`）；统一失败信息（:130-131）；不暴露种子（:83） | n/a（无 E2EE 信道） | n/a | n/a | n/a（TLS + 同网） |
| **controller（桌面/移动 SPA 共用）**<br>`packages/remote/src/shared/cloud/controllerCloudProvider.ts` | 发送侧：无 0x02 时明文 totp-verify | 发送侧：收到 0x02 后改发 totp-bind（:387-388） | 发起 trust 握手 | ✅ 验签 `verifyIdBindSignature`（:370），失败 → $/bye + 断开（:374-377）；**TOFU 指纹变化仅告警不拒**（:379-386，P3 翻闸） | ✅ `decideKeyBinding`（:409）；**宽限过期 → relay-trust**（:420,:431-438）；绑定未决前不放行业务帧（:334） |

## 2. 桥级默认语义（`cloudHostBridge.ts`，全部有测试钉死）

| 条件 | 行为 | 测试 |
| --- | --- | --- |
| 两种 TOTP 校验器都未注入 | **不门控**（verified=true，向后兼容路径；生产无此构造点） | `:446,:631` |
| 仅 totpBindVerifier（bind-only） | 明文 totp-verify 恒失败，无降级 | S1 新增 |
| 仅 totpVerifier | totp-bind 恒失败（除非全未注入） | `:621` |
| totp 失败 ≥5 次 | 锁死本连接 TOTP 通道（含 trust-proof 共享计数） | `:471,:604` |
| keyBindingVerifier 未注入 | `verifyPeerKey` 恒 true（relay-trust） | `:337` |
| bindTranscript 未注入 + trust-proof | **非直接失败**：退化为 prefix‖nonce 无信道绑定签名，由 `totp_trust_check` 裁决 | S1 新增（钉死） |
| transcript 不对称（controller 有、host 无） | 签名不匹配 → trusted:false | S1 新增 |

## 3. 回落面清单（按退役优先级）

| # | 回落面 | 现状风险评估 | 退役条件（fail-closed 目标） |
| --- | --- | --- | --- |
| F1 | trust-grant 无 transcript 退化（桥级） | 低-中：仍需 Ed25519 私钥 + 已在 host 信任库；但失去信道绑定，理论可被中继 | host 侧要求 bindTranscript 必在才接受 trust-proof；先加遥测确认无旧端走此路径 |
| F2 | 身份绑定宽限期 relay-trust（B3，双侧 3s） | 中：DataChannel MITM 无法阻止信令公钥到达（独立 TLS），逃逸需同时控制信令；为旧端兼容而存在 | 统计 0x01/0x02 与 enforced/relay-trust 占比；旧端占比 ~0 后把宽限回落改为拒绝 |
| F3 | TOFU 指纹变化仅告警 | 中：换机合法场景与 MITM 不可区分，故本期不强拒。**计数已实施**（iteration 8：controller `bindingCounters.tofuChanged` + 测试钉死） | UI 提供「确认新指纹」流程后改为默认拒绝待确认 |
| F4 | 桌面 host 无身份密钥 → 0x01 | 低：仅影响无 DPAPI 身份的旧安装。**计数已实施**（iteration 8：host `bindingCounters.fallback0x01`，含签名失败降级路径 + 测试钉死） | 设备身份密钥迁移完成后 0x01 仅限 LAN；cloud 要求 0x02 |
| F5 | ~~`keyBindingVerifier` 钩子全生产未接（§5.5）~~ | **已退役（删除，iteration 8）**：codegraph+grep 确证生产零接线（唯一构造点 cloudHostStore.ts 未注入、`verifyPeerKey` 无生产调用方）；其目标场景由 0x02 + B3 覆盖。删除物：桥 config/字段/`verifyPeerKey`、`makeKeyBindingVerifier`、对应测试 | 已达成（减法路径） |
| F6 | 桥「双校验器未注入 = 不门控」默认 | 理论：生产三构造点均注入；仅测试/未来新构造点可能踩中 | 新构造点须过 S1 门禁测试；长期可改为构造时必传其一 |

## 4. 与合同的对应

- 门禁测试：`cloudHostBridge.test.ts` "S1 兼容回落面" describe（3 例）+ 既有 §4/§5.5/§7.4 套件（56/56 绿，2026-07-23）。
- 遥测与退役设计：见 `docs/superpowers/specs/2026-07-23-s1-fallback-telemetry-retirement-design.md`。
- 本轮明确不做：翻 fail-closed 开关、实现产品遥测、改任何回落行为。
