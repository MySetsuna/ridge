# CONTRACT — iteration 13

日期：2026-07-23
主题：P2 阶段 2 实现——远端 HITL 裁决通道（按已定稿设计 `2026-07-23-hitl-resolution-v2-design.md`）

## 解冻授权存照

维护态冻结由用户指令解除（2026-07-23「按你审理解继续推进」）：存量愿景余项恢复推进。**Level 2 不变**：分支提交、人工审查合并；审查导读每轮刷新以控债务。协议面扩张限本设计范围（`teammate` 能力下一个新 mutating 方法）。

## 自动轨目标

### G1[P2·主线] 裁决通道 Rust 核心

- `PendingEntry` += `nonce`（随挂起项生成，uuid v4 simple 122bit 不可猜）；`list_pending` 投影 += `resolutionNonce`（六字段，仍绝不含 `action`）。
- 新 `resolve_remote(id, nonce, verdict)`：同锁内查条目→**恒时比对** nonce→一致才取出（单次消费原子化）；verdict 仅 approve/reject（**modify 永不开放**）；返回 outcome ∈ `consumed | already-resolved | nonce-mismatch | bad-verdict`；超时后到达 → already-resolved 无副作用。桌面 `resolve`（含 modify）不变。
- 验收：Rust 测试——错 nonce 拒且条目存活；对 nonce 恰消费一次、二次 already-resolved；bad verdict 拒；投影六字段无 action。

### G2[P2] 六处宣告 + 路由

- `resolve_hitl_remote` 入 Rust `REMOTE_ALLOWLIST` + `MUTATING_METHODS` 及 TS 双镜像（同位保 parity）；`REMOTE_CAPABILITY_METHODS.teammate` += ；LAN 路由臂；tauri 命令注册；矩阵 teammate methods += 。
- 负断言维持：`resolve_hitl_request`（桌面版，含 modify）/`set_hitl_enabled`/`suspend_agent`/`resume_agent` 仍不可远达；新增正断言：`resolve_hitl_remote` ∈ MUTATING_METHODS 双侧。

### G3[P2] RemoteLink 双实现 + UI

- `HitlPendingItem` += `resolutionNonce`；`resolveHitlRemote(id, nonce, verdict)` LAN（invoke-request 信封）与 cloud（invoke）双实现。
- Team 面板 Pending 区加 Approve/Reject 双按钮；`already-resolved`/`nonce-mismatch` 给一行反馈后随轮询消隐；无二次确认对话框。

### G4 门禁与导读

- 全门禁绿 + 闭环刷导读。

## 自动验收

```powershell
cargo test --workspace
pnpm exec vitest run packages/remote/src/shared/
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
node scripts/generate-review-pack.mjs
```

## 明确不做

- 远端 modify；审批委托/批量裁决；审计持久化（属 iteration 14 M1 切片二 + M2 合并轮，接 decisions 管道）；G1 阶段二。
