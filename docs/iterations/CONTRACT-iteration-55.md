# CONTRACT — Iteration 55 / AC4-C55（约 2 日 · 协议准入 + 能力边界）

**Credit ID**: C55 · OP-PROTO-DOC/CAP 加厚

## 产品结果
protocolAdmission TS 镜像；protocol_guard catalog/batch/category；capability_matrix_guard full_surface；desktop-only 脚本。

## 独占主文件
- `packages/remote/.../protocolAdmission.ts`(+test)
- `packages/ridge-core/src/protocol_guard.rs`
- `packages/ridge-core/src/capability_matrix_guard.rs`
- `scripts/check-desktop-only-hosts.mjs`

## 验收
vitest protocolAdmission；cargo protocol_guard + capability_matrix_guard；node scripts/check-desktop-only-hosts.mjs。

## 停机
第二协议 SSOT。
