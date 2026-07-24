# CONTRACT — Iteration 54 / AC4-C54（约 2 日 · Live 泵背压策略）

**Credit ID**: C54 · OP-BP-GUARD 加厚

## 产品结果
livePumpPolicy 批处理/公平排序/徽章；hosts inject_live_output 丢弃计数；OutboundStatsDto liveDroppedBytes。

## 独占主文件
- `packages/remote/.../livePumpPolicy.ts`(+test)
- `packages/remote/.../liveBackpressure.ts`
- `src-tauri/src/hosts/mod.rs` inject_live_output
- `src-tauri/src/hosts/desktop_surface.rs` OutboundStatsDto

## 验收
vitest livePumpPolicy + liveBackpressure；cargo hosts:: live_output_cap。

## 停机
cgroup 内存条。
