# CONTRACT — iteration 5

日期：2026-07-23
主题：可信基线固化——安全回落审计（S1 主线）+ 生产状态只读证据（T3）+ 双仓 cargo 绿灯（T1）+ 能力矩阵机器可读化（A2）

## 目标（按序推进，互不强依赖）

### G1[主线·S1] 安全回落构造点矩阵 + 门禁测试 + 遥测/退役设计

- 产出 `docs/security/cloud-fallback-matrix.md`：枚举每个 host/controller 构造点（desktop host `cloudHostStore.ts`、CLI host `ridge-cli`、LAN host、cloud controller desktop/mobile）× 每个 verifier（明文 TOTP、totp-bind、trusted-controller、设备身份 0x02、E2EE pubkey 绑定）的注入状态、回落条件与 `file:line` 证据。
- 新增确定性测试：`CloudHostBridge` 在「verifier 注入」与「verifier 缺失」两种构造下的门控/回落行为逐项断言（含：缺 totpVerifier 时的放行面、缺 bindTranscript 时 0x01 回落语义）。
- 遥测（握手版本/绑定模式/回落原因计数）与退役条件只出设计文档 `docs/specs/2026-07-23-s1-fallback-telemetry-retirement-design.md`，不实现产品遥测、不翻 fail-closed 开关。

### G2[T3] 生产两条版本线只读状态证据

- `ridge-cloud`：新增 `GET /api/v1/remote-artifacts/status`（`RIDGE_ARTIFACT_TOKEN` Bearer 守卫）：返回 current 版本、manifest（version/gitSha/builtAt）、保留 releases 列表；不改动上传/回滚行为；配 cargo 单元测试（含未授权 401/403 路径）。
- `wind`：新增 `scripts/check-prod-status.mjs`：一次调用输出 cloud `GET /api/v1/health` + artifact status 两条版本线汇总；支持 `--base-url`/token 环境变量；无凭据时明确报「未验证」而非伪造绿。
- 对生产环境实跑由用户执行（凭据与网络在用户手中），脚本以本地/单测桩验证。

### G3[T1] 双仓 cargo 绿灯证据

- `wind`：`cargo test --workspace` exit 0（已启动，结果记入报告；若红则只修首因或如实记录，不宣称绿）。
- `ridge-cloud`：`cargo test` exit 0。

### G4[A2] 能力矩阵机器可读化

- 产出 `docs/capability-matrix.json`：能力 × 入口（desktop / LAN / cloud desktop / cloud mobile / rdg host / rdg controller）→ supported / denied / degraded / not-applicable，并指向守卫测试。
- 新增一致性测试：矩阵与 canonical 声明（`ridge-core::REMOTE_ALLOWLIST`、`capabilityContract.ts::REMOTE_CAPABILITY_METHODS`）互证，防止矩阵成为第二事实源。

### G5[A1·收窄支线] 共享内核减法审计

- 产出重复 handler 审计报告（按调用图列出 wind 内仍双路径实现的 workspace/pane/Git 面）。
- 至多执行一个「有等价共享路径 + 既有回归覆盖」的示范删除；不满足条件则只出报告不删码。

## 自动验收

```powershell
# wind
cargo test --workspace
pnpm exec vitest run packages/remote/src/shared/cloud/ packages/remote/src/shared/transport/
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
node scripts/check-prod-status.mjs --help   # exit 0，含未验证语义说明
# ridge-cloud
cargo test
```

矩阵/审计文档以其一致性测试、引用 `file:line` 可核为验收；设计文档以评审通过为验收。

## 停机条件

- cargo 任一仓红且首因非本合同改动引入：如实记录、不宣称绿，其余目标继续。
- T3 若被发现需要新增写路径、用户标识或隐私授权：立即停止该目标另起设计。
- S1 测试若暴露可复现安全缺陷：本轮转为单缺陷复现与修复，其余目标顺延。
- A1 无满足条件的删除对象：只出报告，不为凑数删码。

## 明确不做

- 不实现产品遥测 RPC/存储；不翻 fail-closed 总开关。
- 不做 Remote 新写路径功能；E1 WebGPU 挂起；不动 Agent roster/P1（留待里程碑 M2）。
- 不部署生产、不改 Dokku 配置；生产实跑证据由用户产出。
- 真机 smoke（R1）不入本合同：保持用户必办件（runbook 已备）。
