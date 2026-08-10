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
