# Wave 77：NLM 下一候选——移动 Remote 软键盘锚定（2026-08-11）

## NLM 结果

Wave76 提交后，沿同一 NotebookLM 对话做只读追问，排除已完成的 Explorer 跨卷事务化移动、Message Hub、Codex `frameId` 单调性与 Kernel 深根生命周期。NLM 返回下一候选：`REQ-REMOTE-SMOOTH-STATE-02`，即 iOS Safari / Android Chrome 软键盘弹出时的视口、终端光标与 IME 输入框锚定。

用户可见风险：键盘弹出后页面被推空或顶部裁切，输入光标落在键盘下方，或滚动/触点坐标令 IME 输入点脱离真实 PTY 光标；后台恢复与跨 workspace 切换还需保持 pane 复合身份和尾帧连续。

## 本地核验

CodeGraph 追踪到现有链路：

- `src/remote/lib/TerminalCanvas.svelte` 通过 `manager.inputAnchorResolved(paneId)` 得到 TUI-aware 光标锚点。
- `packages/remote/src/shared/terminal/imeAnchor.ts` 的 `activateIme` 固定执行 `scrollToBottom → positionAtCursorOrCenter → focus`。
- `src/remote/lib/keyboardOffset.ts` 以 `visualViewport`、光标/输入框位置和安全间距计算有界负向视觉位移，不改 canvas、容器高度或 PTY rows/cols。
- `TerminalCanvas` 的 pointer/touch 路径不向 IME 锚点传递触点坐标；无效光标回退到实际可见终端区域中心。

聚焦验证：

- `src/remote/lib/keyboardOffset.test.ts` 与 `TerminalCanvas.test.ts`、`imeAnchor.test.ts`、`manager.test.ts`：52/52。
- `pnpm test`：215 files，1978 passed，1 skipped。
- `pnpm exec svelte-check --tsconfig ./tsconfig.json`：0 errors / 0 warnings。

结论：本地确定性实现已存在，当前没有足够证据指向新的源码根因；不新增第二连接、Query cache、GoalStore、GraphState 或 Postgres 架构。

## 仍需现场证据

- iOS Safari、Android Chrome 真机记录 `visualViewport`、容器/输入框 `DOMRect`、键盘开合前后 PTY rows/cols 与 grid 不变。
- 真机中文/英文 IME、缩放、非底部唤起回底、快速切 workspace/pane、PWA 后台 15 分钟恢复；验证无重复 TOTP、尾帧连续、取消/重连计数归零。
- 本轮 `cdp-remote-mobile-agents.mjs` 首次因未配置 `RIDGE_REMOTE_CA_CERT` 触发自签 CA 校验失败；补充本机 CA 后仍未在 120 秒内收敛。该结果登记为 mobile TLS/harness 现场阻塞，不冒充产品失败或真机验收。

Wave77 本地未产生代码改动；Wave76 已提交为 `35f22196`。发布、push、tag、Release 未执行。
