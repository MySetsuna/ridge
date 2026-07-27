# CONTRACT — Iteration 61 / Remote-Agent 贯通（约 2 日）

需求：`REQ-REMOTE-01/02`、`REQ-CLOUD-01`、`REQ-MOBILE-01`、`REQ-AGENT-01/02`  
设计：`docs/specs/2026-07-27-rdg-remote-agent-center-integration-design.md`

## 目标 1：rdg 显式 Remote 控制面

- 改 `packages/ridge-cli/src/tui/dashboard.rs`。
- LAN root URL；默认 stopped；LAN/public Remote 菜单显名；生命周期复用现有实现。
- 验收：dashboard 纯逻辑/状态测试；`cargo test -p ridge-cli`。

## 目标 2：LAN desktop-web 直连

- 改 `src/routes/+layout.svelte`，新增可单测 boot-classifier。
- LAN origin 不访问 cloud bootstrap；接入现有 LAN adapter/bridge。
- 验收：classifier tests + desktop-web build + LAN probe/同构集成。

## 目标 3：ridge-cloud 配额根修

- 跨仓改 migration、device repo/quota、WS handler。
- 按真实用户组；quota/manual park 分因；只恢复 quota park。
- 验收：`cargo test` 全绿；新增纯逻辑与 repo/handler 回归。

## 目标 4：Mobile Remote UI

- terminal/workspace popup portal；team Bot；Agent icon-only；右上功能按钮无 border。
- 保留 tooltip、aria 与触控尺寸。
- 验收：Svelte/Vitest + mobile/desktop-web build + Playwright 结构断言。

## 目标 5：全局 Agent Center 与历史

- 聚合全部 `workspacesList` 的 topology；写操作使用所属 workspace。
- header 控件下移 content；前台进程自动注册并同步 pane header。
- 有界解析 Claude/Codex JSONL assistant reply。
- 验收：聚合/识别 Vitest；Rust JSONL fixtures；pane layout 状态回归。

## 目标 6：Agent-owned headless shell

- 传播 creator workspace/pane 元数据到 ridge-tmux native session。
- Agent Center 按 agent 嵌套显示，未归因单列；复用 summon 落当前工作区。
- 验收：ridge-tmux header→DTO 测试；tmux shim header 测试；Agent Center attach 测试。

## 允许修改

- `packages/ridge-cli/**`
- `packages/ridge-tmux/**`
- `packages/remote/**`
- `src/remote/**`
- `src/lib/{components,stores,teammate}/**`
- `src/routes/+layout.svelte`
- `src-tauri/src/{bin,commands,teammate}/**`
- 本轮 docs/tests
- `C:\code\ridge-cloud\src/**` 与顺序 migration

## 禁止

- Remote 协议副本、绕过 TOTP/E2EE/设备归属。
- 发布、push、生产数据修改。
- 删除 NotebookLM 来源或开放愿景 note。
- 覆盖既有 `Cargo.lock` 用户改动。

## 总闸

- wind：聚焦测试 → `pnpm check` → Rust 相关包 → 双 Remote build。
- ridge-cloud：`cargo test`。
- `requirements_gate.py assert-executable` exit 0。
- `git diff --name-only` 仅命中允许路径；每个 concern 独立提交。
