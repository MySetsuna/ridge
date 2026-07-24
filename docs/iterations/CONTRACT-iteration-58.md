# CONTRACT — Iteration 58 / AC4-C58（约 2 日 · 出站生命周期状态机）

**Credit ID**: C58 · OP-WS-PTY/LIFE 加厚

## 产品结果
outboundLifecycle 纯状态机（hello→live→detach）；idempotent subscribe；跨 host fanout 守卫；Rust outbound/lan 回归。

## 独占主文件
- `packages/remote/.../outboundLifecycle.ts`(+test)
- `src-tauri/src/hosts/outbound.rs`
- `src-tauri/src/hosts/lan_transport.rs`

## 验收
vitest outboundLifecycle；cargo hosts::outbound。

## 停机
前端多 WebRTC 出站。
