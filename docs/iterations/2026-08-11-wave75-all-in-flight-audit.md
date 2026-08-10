# Wave 75：全部在途需求当前审计（2026-08-11）

## 范围与门禁

- 当前请求已绑定 `INTAKE-20260811-ALL-IN-FLIGHT-01`；包含 `docs/REQUIREMENTS-SPEC.md` 中 60 条 `ACTIVE` 需求。
- `requirements_gate.py assert-task-executable`：`executable=true`，`pending_ids=[]`，`active_missing_headings=[]`。
- 本轮继续保持单仓 `main` 工作流；未 push、未 tag、未 Release，未向 Codex 之外 CLI、Agent 或 teammate 发消息。

## 当前代码事实

| 需求面 | 当前可复核证据 | 结论 |
|---|---|---|
| SCM Git 深度/Pane branch pill | `find_git_repos_below_sync` 将向下扫描硬封顶为 1；`paneGitStatus` 只接受后端 `find_git_repo_root(cwd)`；Rust scan tests 与 `paneGitStatus.test.ts` 覆盖根归属、深层仓库、非 Git 父目录 | 本地代码与确定性测试已闭合；真机现场仍按需求边界保留 |
| History/Agent catalog | `read_agent_recent_replies_sync` 按 Agent/session 聚合，已有 Claude、Codex、Grok adapter；Grok `summary.json` + `chat_history.jsonl` 有解析测试；恢复使用结构化 argv | 本地实现已存在；真实第三方 CLI 格式/现场仍不冒充完成 |
| History overlay/Terminal geometry | `history_overlay_geometry` 对 pane-local viewport 做翻转、夹紧、窄宽居中与行数收缩；DPR fixture 已覆盖；Remote pane geometry 共享同一网格投影 | 纯逻辑证据已闭合；原生 PowerShell/物理 DPR 捕获仍外部 |
| Agent Hub/PTy safety | Kernel roster identity、generation/lease、Hub envelope、Desktop Tauri identity read 与五条件快照发布已接线；旧证明在释放/替换时清理，缺采样仍 fail-closed | 安全边界已闭合；第三方 Runtime/A2A 与真实 Agent CLI 仍未宣称兼容 |
| Remote Host retry | 新增 store 层 `topologyRetryInFlight` 单飞；取消会 abort 当前 generation、清除 retry reservation，后续重试可重新建立；重复 retry 回归 `11/11` | 本地单飞/取消边界已闭合 |

## 当前质量证据

- `pnpm test:coverage:sonar`：215 files，`1975 passed / 1 skipped`，exit `0`；本地 LCOV statements `14448/21901 = 65.96%`，仅作诚实本地基线。
- `pnpm check`：`0 errors / 0 warnings`。
- `cargo test -p ridge-core --lib --quiet`：`335 passed / 0 failed`。
- `cargo test -p ridge-kernel -p ridge-mcp --lib --quiet`：`49 + 93 passed / 0 failed`。
- `cargo fmt --all` 通过；Windows linker informational warning 不影响退出码。
- Sonar 权威项目 API 最新分析 `9abdb231-3503-4f6e-b8ee-48d28f7086dc`：coverage `80.4%`、line `86.7%`、branch `71.5%`、Quality Gate `OK`、`new_violations=0`、`new_issues=0`。
- Sonar 总体仍有 1 条历史 `Web:PageWithoutTitleCheck`；当前 `src/remote/index.html` 已有 `<title>`，需后续一次成功上传分析刷新旧 issue。不能把“Gate OK”误写成“总 issue 数为零”。

## 尚未取得的硬证据

以下不是本地单测可替代项，仍保持 ACTIVE，未作完成宣称：

1. 成功上传当前源码后的 Sonar 全源复扫及历史 issue 清零确认；此前重扫曾受 Windows JS bridge/Node heap 与本地任务回收影响。
2. Cloud/Postgres 真实凭据链、公网四路径、实体手机 clean profile、WebView2 长跑、双窗口/双 Host 与第三方 CLI Runtime/A2A。
3. 原生 PowerShell/ConPTY 粘贴与渲染录制、物理 DPR `1/1.25/1.5/2` 捕获、跨卷 ACL 中窗。

## 本波变更

- `src/lib/stores/hosts.ts`：Host 拓扑重试增加 store 级单飞和取消后重试语义。
- `src/lib/stores/hostsPublic.test.ts`：重复重试与取消后重试回归。
- `src-tauri/src/teammate/mcp.rs`：按 `cargo fmt` 规范化当前身份围栏测试。

本文件只记录本地审计事实与证据边界；生成的 coverage、`.iteration`、E2E 截图/结果未纳入提交。
