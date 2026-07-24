# CONTRACT — Iteration 49 / AC4-C10（约 2 日 · Protocol admission 全链）

**Credit ID**: C10

## 产品结果

方法 canonicalize / desktop privileged 分类 / remote admit：**dispatch** 桌面入口、**remote_host_impl** 远端入口、**cloudHostBridge** 云 invoke 双门（protocol + allowlist）、TS remoteInvokeAdmit 包装。

## 独占主文件（多模块）

- `packages/ridge-core/src/protocol_guard.rs`(+catalog/batch tests)
- `packages/ridge-core/src/dispatch.rs` admit_desktop_method
- `src-tauri/src/remote_host_impl.rs` admit_remote_method
- `packages/remote/src/shared/transport/protocolAdmission.ts`(+test)
- `packages/remote/src/shared/transport/remoteInvokeAdmit.ts`(+test)
- `packages/remote/src/shared/cloud/cloudHostBridge.ts` decideRemoteInvoke 接线
- `src-tauri/src/hosts/desktop_surface.rs` + `scripts/check-desktop-only-hosts.mjs`

## 验收

| # | 信号 |
| --- | --- |
| 1 | `cargo test -p ridge-core --lib protocol_guard` + dispatch 测 |
| 2 | vitest remoteInvokeAdmit + protocolAdmission |
| 3 | `node scripts/check-desktop-only-hosts.mjs` exit 0 |

## 停机

空 Release；第二 SSOT。
