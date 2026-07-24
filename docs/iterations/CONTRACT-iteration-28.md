> **SUPERSEDED for AC4 accounting** — 原拆账不计 10 分。诚实 credit 见 `CONTRACT-iteration-40`…`49` 与 `{SCRATCH}/ac4-ledger.md`。
# CONTRACT — Iteration 28（约 2 日 · OP-CAP-PARITY）

## 范围边界

能力边界与 desktop-only 主机方法表；脚本门禁；非出站状态机。

## 目标（6）

1. DESKTOP_ONLY_HOST_METHODS 含 pump/bind_mock/get_outbound_stats。
2. capabilityContract：REMOTE_ALLOWLIST 拒绝全部 desktop host 方法。
3. check-desktop-only-hosts.mjs 交叉 rust/ts allowlist。
4. desktop_surface 单测 no overlap with remote surface。
5. is_desktop_only_host_mutating 分类。
6. OutboundStatsDto 序列化形状稳定。

## 验收

| # | 信号 |
| --- | --- |
| 1 | vitest capabilityContract 绿 |
| 2 | node scripts/check-desktop-only-hosts.mjs exit 0 |
| 3 | cargo test desktop_surface 绿 |

## 代码面

`desktop_surface.rs`、capabilityContract.test.ts、check-desktop-only-hosts.mjs。

