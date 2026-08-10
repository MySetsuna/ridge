# NLM 需求闭环交接（2026-08-11）

## 结论

未全部闭环。仓内可确定复现、可本地验证部分已落地；仍有外部凭证、宿主权限、物理设备与第三方协议证据缺口，故不宣称 NLM 全部需求完成。

## 已落地

- Agent 通信：`AgentEnvelope::validate` 现在校验双方完整身份、workspace 一致性、可选关联 ID 与非零 sequence；新增 7 类畸形输入测试。
- Workspace memory：前端新增 Tauri sidecar 读写桥，编组 store 在切换/持久化时恢复并镜像 goal、constraints、tasks；解析对 IPC 数据做防御性归一化。
- Remote 开发入口：`vite.remote.config.js` 固定 HMR `ws` 与 `clientPort: 5174`，避免旧端口缓存导致 remote 页面连不上。
- ridge-term：修正既有 RenderBackend 拆分造成的重复 trait impl、helper 归属、glyph style 类型与宽字符 span 类型错误；Canvas2D/WebGPU 均恢复单一 trait 适配实现。
- tmux/cloud：修复 send-keys 代码边界导致的编译阻断，以及 `Instant` 借用解引用错误。

## 证据

- `pnpm test`：214 files，1965 passed，1 skipped。
- `pnpm check`：0 errors，0 warnings。
- `cargo test -p ridge-core --lib teammate::communication`：6/6。
- `cargo test -p ridge-term --lib`：399/399。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`：272/272。
- CDP：真实桌面构建、WASM 构建完成，`CDP ready on port 5571`；smoke、DPR、teammate 7/7、跨卷 ACL 通过。

## 未闭环 / 下一轮缺口

1. PTY 五条件运行时快照尚无生产采样器与原子证据，当前保持 fail-closed，不把普通 pane 状态冒充 PTY 安全状态。
2. NLM 提到的 `GoalStore`、`GraphState`、Postgres checkpointer 在本仓无实现或调用契约；当前只落地已有 workspace-memory sidecar，不擅自引入无上游协议的新架构。
3. 真实 CDP 中内核 detached process 因 Windows `CREATE_BREAKAWAY_FROM_JOB` 权限拒绝，导致 pty/remote pane 场景失败：
   `spawn detached process requires CREATE_BREAKAWAY_FROM_JOB for force-kill survival: 拒绝访问。 (os error 5)`。
4. `rdg-remote-e2e` 默认 HTTPS 证书链校验失败；cloud full E2E 缺 user/device/cloud credentials；teammate MCP E2E 缺 sidecar endpoint。均未伪造通过。
5. SonarQube `http://127.0.0.1:9000` 状态为 UP。scanner 产生了 `.scannerwork/report-task.txt`，项目为 `MySetsuna_ridge`；但当前环境无可用 `SONAR_TOKEN`，quality-gate/CE 查询返回 `401`，故没有把本地覆盖率或提交记录冒充 Sonar Gate 结果。密码未写入仓库、文档或日志。
6. 物理手机 HTTPS/IME/DPR、第三方 Runtime/A2A 协议互操作仍需真实设备或外部凭证；本轮仅完成仓内边界与 fail-closed 逻辑。

## 认证与安全边界

NotebookLM 已按固定代理 `http://127.0.0.1:51081` 完成重新认证并通过 `nlm login --check`；认证材料未输出、未写入本仓。发布、push、tag、release 均未执行，保留人工授权边界。

## 可复验命令

```powershell
pnpm test
pnpm check
cargo test -p ridge-core --lib teammate::communication
cargo test -p ridge-term --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib
$env:CDP_PORT='5571'; node scripts/cdp-smoke.mjs
$env:CDP_PORT='5571'; node scripts/cdp-dpr-e2e.mjs
```

## 最新复核（2026-08-11）

- PTY OSC 0/1/2/7 已增加跨读取 carryover；普通输出即时下发，未完成元数据有 64 KiB 上限，EOF 与读错路径均 flush。新增 4 个单测，`cargo test -p ridge-core --lib pty::osc_stream` 通过。
- 真实 Tauri/CDP 重启后，PTY parser E2E 已验证 UTF-8、OSC 2 标题、OSC 7 CWD；移动端 Remote agent E2E 已验证双 device-bound 会话、workspace snapshot、pane layout、TLS/WS、移动 SPA 与 headless 能力降级，`GATE: PASS`。
- `rdg-remote-e2e` 在显式信任本机 Ridge CA（`%LOCALAPPDATA%\ridge\remote-tls\ca.pem`）后通过；未安装/未指定本机 CA 的 Node 客户端仍会按 TLS 安全策略拒绝自签证书，不能用关闭校验冒充修复。
- DPR E2E 默认 app-ready 等待已提高至 180 秒，以覆盖 Windows 冷启动编译，不改变生产超时语义。

仍未闭环：SonarQube 当前 API 无可用 token 时返回 `401`，故未宣称本轮质量门；PTY 五条件运行时采样、真实手机 HTTPS/IME/DPR、第三方 Runtime/A2A、Cloud/物理设备凭证仍属外部验收；`GoalStore`/`GraphState`/Postgres checkpointer 未在本仓形成可验证合同，未擅自引入新架构。

## 最终复核（2026-08-11）

- Remote 元数据闭环：`rdg` sidecar 订阅现按 pane 维护增量 UTF-8/OSC carryover，转发 `pty-meta`；桌面 Remote 原始流亦有同等兜底。拆包 title/CWD 单测通过，真实 PTY E2E 的 UTF-8、OSC2、OSC7 均 PASS。
- Remote 路由：stdin 优先使用消息 `workspaceId`，缺失/非法时才回退连接 workspace；对应 2 个单测通过。
- E2E：smoke、DPR、跨卷 ACL、teammate、term input、LAN probe、PTY parser、移动 Remote `GATE: PASS`、独立 `rdg-remote-e2e ALL PASS`。自签 TLS 仅通过显式 `%LOCALAPPDATA%\ridge\remote-tls\ca.pem` 验证，未关闭证书校验。
- 质量门：`pnpm test` 214 files / 1965 passed / 1 skipped；`pnpm check` 0 errors / 0 warnings；Rust 定向测试 `ridge-core` 4、`ridge-cli` 15、Tauri remote 16、Tauri PTY 2 全通过；`cargo fmt --all -- --check` 通过。
- SonarQube 服务仍为 UP；因当前环境无 `SONAR_TOKEN`，API/quality-gate 查询返回 `401`，本轮不宣称 Sonar Gate 通过。接管入口与凭证边界见 `docs/iterations/2026-08-09-sonarqube-handoff.md`，密码不落仓。

本轮仍不宣称 NLM 需求“全部”完成：真实物理设备、Cloud/第三方凭证、PTY 五条件生产采样，以及仓外 `GoalStore`/`GraphState`/Postgres checkpointer 合同，仍无可验证证据。
