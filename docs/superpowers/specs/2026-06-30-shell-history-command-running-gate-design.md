# 设计：shell-history 弹层门控扩展到「进程内命令」（OSC 133;A prompt 标记）

日期：2026-06-30
状态：设计→实现
承接：[2026-06-27 fix] 前台有进程运行时禁用 shell-history 弹层（`get_pane_foreground_process` 轮询门控）

## 背景与问题

2026-06-27 已落地：命令运行时（前台有 **OS 子进程**）禁用 shell-history 弹层，用 `get_pane_foreground_process`（枚举 shell 子进程）做 active-pane 轮询门控。

**遗留边界**：该信号只检测 OS 子进程。PowerShell 的**进程内 cmdlet**（`Start-Sleep`、纯 PS 循环、`Measure-Command { ... }` 等）不 fork 子进程 → 检测不到 → 命令运行中按 ArrowUp 仍会弹 history。同理任何不 fork 子进程的内建命令。

目标：命令运行中（无论是否 fork 子进程）一律禁用 shell-history 弹层。

## 为什么需要 shell 集成

要区分「shell 空闲在 prompt 等待输入」与「正在执行命令（含进程内 cmdlet）」，唯一可靠的信号是 **shell 自己上报命令边界**（FinalTerm/VS Code OSC 133 协议）。进程枚举做不到（`Start-Sleep` 与空闲 prompt 在 OS 层无差别：都是 powershell.exe 阻塞、低 CPU、无子进程）。

ridge 已有的 shell 集成框架（`src-tauri/src/commands/terminal.rs`）目前只在 prompt 钩子里发 **OSC 7（cwd）**：
- PowerShell：`PS_INIT` 包装 `prompt` 函数（`-NoExit -EncodedCommand`）。
- bash：`PROMPT_COMMAND` 环境变量。
- zsh：ZDOTDIR 技术 + `precmd` 钩子。
- cmd.exe：无 hook 机制（不支持）。

> 注：项目此前刻意只移植 cwd（command-status 未移植）。本设计**只新增一个 prompt 标记 `133;A`**，不引入 preexec 钩子、**不覆写 PowerShell Enter 键**——把"命令是否在跑"的判定放到前端，规避 shell-breaking 风险。

## 设计（最小、低风险）

核心：**prompt 钩子额外发 `OSC 133;A`（prompt start），前端用「Enter 提交 → 等下一个 prompt」括出"命令运行中"窗口。**

### 数据流
1. shell 每次渲染 prompt 时发 `\e]133;A\a`（在现有 OSC 7 同一位置追加）。
2. 后端读线程已有的 `find_prompt_osc`（`ridge-core/pty/prompt.rs`）检测 `133;A` → 经 `chunk::process` 置 `prompt_seen` → emit `GlobalEvent::PanePromptDetected` → `lib.rs` 转发 Tauri 事件 `pane-prompt-{ws}-{pane}`（**已存在，当前前端无消费者**）。
3. 前端 `RidgePane`：
   - 监听 `pane-prompt-{ws}-{pane}` → `hasShellIntegration = true; commandRunning = false`（回到 prompt = 空闲）。
   - 在 shell 路径（`!isTui`）按下 **Enter**（`dispatchBufferEvent` 的 `'clear'`）→ 若 `hasShellIntegration` 则 `commandRunning = true`（提交命令 = 运行中，直到下一个 prompt 事件复位）。
   - history 弹层门控加 `&& !commandRunning`。

### 为什么这样安全/正确
- **零 shell-breaking**：只多发一个不可见 OSC（和 OSC 7 同款写法），不动按键绑定、不加 preexec。
- **不会卡死**：`commandRunning=true` 仅在 `hasShellIntegration`（已收到过 prompt 事件）时设置。cmd.exe / 集成缺失 → 永不收到 prompt 事件 → `commandRunning` 永远 false → 回退到 `get_pane_foreground_process`（外部进程仍被挡）。
- **覆盖进程内 cmdlet**：`Start-Sleep 30` → Enter 置 true → 30s 内无 prompt → 持续挡 → 完成后 prompt 事件复位。✓
- **覆盖 REPL/子 shell**：进入 `python`/`node` REPL 后我们的 shell 不再发 prompt → `commandRunning` 持续 true → 挡 host history（符合预期，REPL 有自己的历史）。退出后 shell prompt 复位。✓
- **与既有信号叠加**：门控 = `!foregroundProcessRunning && !commandRunning && shouldAllowShellHistory`。集成在时 `commandRunning` 足以覆盖一切；集成缺失时 `foregroundProcessRunning` 兜底外部进程。

### 已知次要边界
- **多行输入**：PSReadLine 在不完整语句上按 Enter 会插入换行（不提交），但前端仍把它当 Enter → `commandRunning=true`，直到真正提交→命令跑完→prompt 复位。期间 history 被挡。罕见、可接受。
- **空行 Enter**：瞬间 true→prompt→false，无害。
- bash/zsh 的 `133;A` 走 `PROMPT_COMMAND`/`precmd`，与现有 OSC 7 同钩子；PowerShell 走 `prompt` 函数。cmd.exe 不支持（按设计回退）。

## 改动清单
1. `src-tauri/src/commands/terminal.rs`：PS_INIT / bash PROMPT_COMMAND / zsh `.zshrc` precmd 各追加 `133;A` 发射。（需重建 ridge.exe 生效；dev:cdp 自动重建）
2. `src/lib/components/RidgePane.svelte`：`listen('pane-prompt-...')` + `commandRunning`/`hasShellIntegration` 状态 + Enter 置位 + history 提交置位 + 门控加 `!commandRunning` + onDestroy 清理。（纯前端）

**不改 kernel/ridge-term、不改 wasm、不新增 Tauri 命令。**

## 验证
- `pnpm check` 0 新错误。
- dev:cdp 真机：①`Start-Sleep 20`（进程内 cmdlet，无子进程）运行中按 ArrowUp **不弹**；②空闲 prompt 按 ArrowUp **弹**；③`get_pane_foreground_process` 对 Start-Sleep 返回 null（确认这是 cmdlet、旧信号确实漏）；④普通命令历史导航正常；⑤PowerShell 正常输入/多行/复制粘贴不受影响。
