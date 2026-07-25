# P4 手机多 kernel 保活 —— 盲改改动/风险台账

> 用户已授权「本轮盲改 P4(仅 svelte-check,不跑 app)」,并要求记录改动点与风险点,
> **万一返工能快速定位**。本档随改随记:每改一处即登记「文件:符号 · 改了什么 · 为何 ·
> 风险 · 回退」。P4 全部 commit 前缀 `feat(remote-mobile): P4`,便于整段 revert。

## 背景

手机 SPA(`src/remote/`)此前用**单例 kernel**:`terminalController.ts`(一个 wasm
`TerminalKernel` + 一块 canvas)+ `paneScrollbackCache.ts`(≤256KB 旁路字节缓存)+
`MainApp.svelte` 的 `resetForSwitch`/`reconcileReplay`/prune 逻辑。切 pane 要清屏重放
→ 白屏、scrollback 脆弱、与桌面渲染核心割裂。

P4 = 改用共享 `@ridge/remote/shared/terminal/manager.ts`(多 kernel + 单 host canvas +
scissor + keep-alive),与桌面 `RidgePane` 同一份。切 pane 只挪 scissor,不 reset/重放/
销毁 → 零白屏;pane 历史活在各自常驻 kernel,旁路缓存退役。

## 无法 headless 验证的风险点(返工时优先核查这些运行时行为)

| # | 风险 | 只有跑 app 才能验 | 若返工,先查 |
|---|---|---|---|
| R1 | 切 pane 白屏/闪烁 | scissor 是否只渲染 active、切换是否零帧空屏 | manager.attach 时 container 尺寸/可见性;active 切换是否触发 RAF 重渲 |
| R2 | scrollback 保真 | 切走→大量输出→切回→上滚 历史是否完整 | 后台 pane kernel 是否仍订阅/feed;LRU 是否误逐出 |
| R3 | 软键盘 offset | `visualViewport` 监听 + keyboardOffset 是否仍作用于新 host canvas | 输入适配层是否正确读 active pane 容器 |
| R4 | IME 组字 | 隐藏 textarea preedit → manager.setPreedit(activePaneId) | preedit 目标 pane 是否为 active |
| R5 | 选择即鼠标(TUI 接管) | 全屏 TUI 里 touch 选择是否仍 encodeMouse 转发 | 适配层是否对 active pane kernel 调 isMouseReporting/encodeMouse |
| R6 | 内存/LRU(低端机) | 开 N+2 pane 是否 OOM、逐出是否生效 | LRU 上限 N、冻结/rehydrate 路径 |
| R7 | 弱网重连 | 断线重连是否黑屏/丢历史 | reconnect 后是否 re-subscribe active pane、kernel 是否保活 |
| R8 | 复制 pill(前修 b5c5a56) | 复制不唤起键盘、TUI 选择归属 | 适配层迁移后 copy pill 事件是否仍 stopPropagation |

## 改动点登记(随改追加)

> 格式:`[commit] 文件:符号` — 改动 · 原因 · 回退

(待实施后逐条填入)

## 回退预案

- P4 各 commit 前缀 `feat(remote-mobile): P4`;整段返工:`git revert <P4 commits>` 或
  `git reset --hard <P4 之前 SHA>`(P4 之前最后一个 commit = 后端收敛完成点,见下)。
- **P4 之前基线 SHA**:`b9031a0`(fix(rdg): live-modes tracking …)——后端收敛全绿、
  上报 bug 已修。P4 全部回退到此仍是可发布的完好版本。
- 退役文件(`terminalController.ts`/`paneScrollbackCache.ts` 及其 `.test.ts`)在 P4 期间
  **先保留、后删**:先接 manager 跑通类型,末阶段再删,便于对照/回退。

## 验证清单(待有 app 环境时补跑)

- [ ] `pnpm svelte-check` 0 err(盲改阶段唯一门禁)
- [ ] `pnpm vitest run` 相关纯逻辑测试绿
- [ ] (app)R1–R8 逐项真机/CDP 验(见上表)
