# CONTRACT — Iteration 48 / AC4-C9（约 2 日 · Capability matrix 全边界）

**Credit ID**: C9

## 产品结果

矩阵 JSON / REMOTE_ALLOWLIST / TS controller methods **三方对账**；desktop host 永不入 teammate；脚本 + cargo + vitest 门禁。

## 独占主文件（多模块）

- `packages/ridge-core/src/capability_matrix_guard.rs`(+shipped matrix test)
- `packages/remote/src/shared/transport/matrixParity.ts`(+test)
- `packages/remote/src/shared/transport/capabilityContract.ts`(+test)
- `scripts/check-capability-matrix.mjs`
- `docs/capability-matrix.json`
- `packages/remote/src/shared/cloud/remoteAllowlist.ts`（对齐）

## 验收

| # | 信号 |
| --- | --- |
| 1 | `cargo test -p ridge-core --lib capability_matrix_guard` |
| 2 | vitest matrixParity |
| 3 | `node scripts/check-capability-matrix.mjs` exit 0 |

## 停机

第二协议 SSOT；扩写未宣告能力。
