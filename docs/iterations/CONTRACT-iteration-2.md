# CONTRACT — iteration 2

日期：2026-07-21  
主题：跨入口 Remote 能力合同门禁

## 唯一主目标

建立可执行合同，使 Remote 的“能力宣告 → 安全 allowlist → 实际 handler → Controller UI”四层一致；新增或迁移能力时，任何入口的静默缺失、错误宣告或无降级 UI 都会让测试失败。

## 范围

- 盘点并复用现有 Rust `REMOTE_ALLOWLIST`、core dispatch、LAN/cloud transport conformance 与 rdg 路由。
- 为 coarse capability（至少 `fs`、`search`、workspace/git 相关能力）声明其必需方法与入口支持状态。
- 加入机器可执行的 contract test，覆盖 LAN desktop host、Cloud desktop host 与 rdg host/controller 的宣告/路由一致性。
- Remote Controller 对未宣告能力隐藏或禁用相应入口，不再无条件展示后等待 RPC 报错。
- 保留 iteration 1 的 Rust canonical ↔ Cloud TS mirror 逐项 parity。

## 边界

- 不新增 RPC，不扩大 allowlist，不暴露本机 MCP endpoint/token。
- 不实现 Remote Agent 控制台、HITL、暂停/恢复。
- 不做弱网功能重构，不顺带修 Vite/PWA 警告。
- 不维护另一份手写全量命令清单；合同数据须复用 canonical 或由测试从实现提取。

## 可验证验收

1. 修复前测试能稳定抓住至少一个当前宣告/路由/UI 不一致；修复后退出码为 0。
2. 每个 advertised capability 的必需方法均在对应入口可路由且被安全放行；否则测试失败。
3. 未宣告 capability 的 Files/Git/Search 等 UI 被隐藏或禁用；禁止入口返回稳定结构化错误，不超时、不静默 no-op。
4. `get_workspace_snapshot`、`git_diff_file` 与 host 特权排除项有明确回归断言。
5. 以下既有闸保持全绿：
   - `pnpm exec vitest run packages/remote/src/shared/cloud/remoteAllowlist.test.ts packages/remote/src/shared/transport/conformance.test.ts`
   - 涉及的 `ridge-core` / `ridge-cli` capability 与 dispatch 定向 Rust 测试。
   - `pnpm build:remote`（先生成 gitignored WASM 前置产物）。

## 停机条件

- 若入口能力语义无法从现有代码唯一推导，先落最小显式 mapping 与争议记录，不用放宽权限掩盖缺口。
- 若发现特权命令误放行，立即转安全修复并停止 UI 工作。
- 若单轮需要改协议或新增 RPC，停止并另起设计合同。

