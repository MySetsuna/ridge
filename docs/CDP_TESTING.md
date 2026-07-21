# Ridge CDP 测试工作流

借助 `tauri:dev:cdp` + `chrome-devtools-tauri` MCP，把**真实 Tauri WebView2 运行时**暴露给自动化工具，从而对 Ridge 做端到端 UI / IME / 终端 PTY / 性能验证 —— 而不是去测一个没有 `window.__TAURI__` 的纯 vite 页面。

---

> ## ⚠️ 重大更新（2026-06-19）：CDP 端口已改为**动态**
>
> **症状（旧）**：本机 WebView2 148/149 上，`pnpm tauri:dev:cdp` 起来后 9222 端口**死活连不上**，无 `DevToolsActivePort` 文件，曾被误记为「WebView2 CDP 坏了/硬件问题」。
>
> **根因（隔离实测确认）**：**Chromium 136+ 安全加固——拒绝固定 `--remote-debugging-port`**（如 9222）。直接拿 Edge 149 验证：`--remote-debugging-port=9223` → 端点静默不开；`--remote-debugging-port=0` → 正常（动态端口写入 `DevToolsActivePort`）。这正是 [Microsoft WebView2 官方推荐做法](https://learn.microsoft.com/en-us/microsoft-edge/web-platform/devtools-mcp-server)（`--remote-debugging-port=0` + 读 `DevToolsActivePort`）。
>
> **修复（`e314895`）**：
> - `scripts/tauri-dev-cdp.mjs` 改用 `--remote-debugging-port=0`，启动后轮询 `<userDataDir>\EBWebView\DevToolsActivePort`，把**真实端口**打印出来并写到 `.webview2-dev-cdp/cdp-port.txt`。
> - `scripts/cdp-port.mjs` 新增 `resolveCdpPort()`：按 `CDP_PORT 环境变量 → DevToolsActivePort → 9222 兜底` 顺序发现端口。
> - 所有自带断言的 `cdp-*.mjs` 探针都经 `resolveCdpPort()` **自动发现**端口，**无需手动指定 9222**。
> - ⚠️ **`chrome-devtools-tauri` MCP 仍写死 `--browserUrl http://127.0.0.1:9222`，动态端口下连不上**——见 §4 的处置（设 `CDP_PORT` 或临时改 browserUrl 到真实端口；或直接用 raw-CDP 脚本，它们自动发现）。
>
> 启动后认准这行（端口每次不同）：
> ```
> [tauri-dev-cdp] ✅ CDP ready on port 9596  →  http://127.0.0.1:9596/json/version
> [tauri-dev-cdp]    attach: CDP_PORT=9596 pnpm cdp:smoke   (or just `pnpm cdp:smoke`)
> ```

---

## 1. 设计动机

| 测什么 | 工具 | 为什么 |
| --- | --- | --- |
| UI / CSS / 布局 / IME overlay | chrome-devtools MCP (attach 9222) | 需要真实 WebView2 渲染管线 + Tauri overlay 注入 |
| 终端 PTY、历史 popup、Shell 输入 | chrome-devtools MCP (attach 9222) | 必须有 `window.__TAURI__`，PTY 通过 IPC 桥接 |
| 控制台错误、网络、Perf trace | chrome-devtools MCP (attach 9222) | 抓真实运行时的 console / `performance.*` |
| 现有 wdio E2E (`tests/e2e-shell/`) | msedgedriver | 保持不变；这是 BDD-style 黑盒驱动 |

`chrome-devtools-tauri` 不替代 wdio，它是 **agent 驱动**的探索性自动化通道：可以让 Claude 真实点击、查 DOM、跑 evaluate、抓截图、复现 bug。

---

## 2. 端口与隔离

`scripts/tauri-dev-cdp.mjs`：

- CDP 端口 **动态**（`--remote-debugging-port=0`，Chromium 136+ 强制）。真实端口由 Chromium 写进 `.webview2-dev-cdp/EBWebView/DevToolsActivePort`（第 1 行），脚本再镜像到 `.webview2-dev-cdp/cdp-port.txt` 并打印。
- 读端口统一用 `scripts/cdp-port.mjs` 的 `resolveCdpPort()`（优先级：`CDP_PORT` 环境变量 → `DevToolsActivePort` → 9222 兜底）。
- 通过 `WEBVIEW2_USER_DATA_FOLDER` 指向项目根的 `.webview2-dev-cdp/`，**与已安装的 Ridge 完全隔离**
- 否则 dev 和正式版共用 `%LOCALAPPDATA%\com.<bundleId>\EBWebView` 会触发 HRESULT `0x8007139F`（ERROR_INVALID_STATE）
- `--remote-allow-origins=*` 仍必带（Chromium 111+ 否则 WS 握手 403）

`.webview2-dev-cdp/` 已加入 `.gitignore`。

> **为什么不能用固定端口**：Chromium 136+ 把固定 `--remote-debugging-port` 当作恶意软件用已知端口附加调试器的攻击面，直接忽略（非默认 `--user-data-dir` 也救不回来）。只有 `=0`（动态）会真正开端点 + 写 `DevToolsActivePort`。`resolveCdpPort()` 的 9222 兜底**只是清晰报错用**，在 136+ 上必失败。

---

## 3. 启动顺序（关键）

chrome-devtools MCP 在 `--browserUrl` 模式下**冷启动时**就会尝试连 9222。Tauri 必须先起来。

```pwsh
# 终端 1 — 启动带 CDP 的 Tauri
pnpm tauri:dev:cdp

# 等输出形如（端口每次不同，由 Chromium 动态分配）:
#   [tauri-dev-cdp] ✅ CDP ready on port 9596  →  http://127.0.0.1:9596/json/version
# 并且 Ridge 主窗口已经显示

# 终端 2 — 探活（自动从 DevToolsActivePort 发现端口，无需指定）
pnpm cdp:smoke
```

`pnpm cdp:smoke` 期望输出（端口为本次动态分配）：

```
[cdp-smoke] connected to 127.0.0.1:9596
[cdp-smoke] browser       : Edg/149.0.4022.80
[cdp-smoke] protocol      : 1.3
[cdp-smoke] targets       : 5
  - [page] http://127.0.0.1:5173/  — Ridge
  - [worker]
  - [worker]
  - [worker]
[cdp-smoke] ridge target : http://127.0.0.1:5173/
```

(Ridge dev 模式下页面是 vite dev server 在 `127.0.0.1:5173`，被 WebView2 加载；不是 `tauri://`。生产构建里才是 `tauri://localhost/`。Worker targets 数量取决于 P4 renderWorker 是否启用。)

退出码 `0` = 链路就绪；`1` = Tauri 没起来或端口被占。

---

## 4. 让 Claude Code 接入

`~/.claude.json` 用户级配置里已经注册了两个 MCP：

| 名字 | 用途 |
| --- | --- |
| `chrome-devtools` | 通用 — 自启 Chrome 浏览器，测任意 web 站点 |
| `chrome-devtools-tauri` | 专用 — `--browserUrl http://127.0.0.1:9222 --experimentalIncludeAllPages`，attach 到 Ridge 运行时 |

> Windows 启动方式（实测 2026-05-22，本机）：**不要用 `npx`**。`npx chrome-devtools-mcp@latest` 每次启动 ~19s（registry 校验 + 解包），远超 Claude-Code MCP host 的启动超时窗口，导致 `chrome-devtools-tauri` 永远"connecting"然后掉线。直接用 `node + 全局安装的绝对路径` ~5s，能稳定上线。前提：`npm i -g chrome-devtools-mcp` 一次性装好。

```json
"chrome-devtools-tauri": {
  "type": "stdio",
  "command": "node",
  "args": [
    "C:/DevKit/nodejs/node_modules/chrome-devtools-mcp/build/src/bin/chrome-devtools-mcp.js",
    "--browserUrl", "http://127.0.0.1:9222",
    "--experimentalIncludeAllPages",
    "--no-usage-statistics"
  ],
  "env": {}
}
```

（路径来自 `npm root -g` + `chrome-devtools-mcp/package.json` 的 `bin` 字段；正斜杠在 Windows Node 上完全支持，避开 JSON 转义坑。）

> ### ⚠️ 动态端口下 MCP 连不上（2026-06-19）
> 上面的 `--browserUrl http://127.0.0.1:9222` 是**固定端口**，而 Chromium 136+ 下端口每次动态变化（见顶部 banner）→ MCP 冷启时连 9222 失败。三种处置：
> 1. **（推荐做 e2e）直接用 raw-CDP 脚本**：`scripts/cdp-*.mjs`（含 `cdp-teammate-e2e.mjs`）经 `resolveCdpPort()` 自动发现端口，**完全不依赖 MCP**，也不受动态端口影响。Node 22+ 自带全局 `WebSocket`，脚本用 `/json/list` 拿 `webSocketDebuggerUrl` 后直接 `Runtime.evaluate`。
> 2. **想继续用 MCP**：起 `tauri:dev:cdp` 后读 `cat .webview2-dev-cdp/cdp-port.txt` 拿到真实端口 N，把 MCP 配置里的 `--browserUrl` 改成 `http://127.0.0.1:N`，再重启 Claude 会话。（端口每次重启 dev 会变，所以每次都要改——不方便，故首选 1。）
> 3. **改用 autoConnect**（Microsoft 推荐）：把 MCP args 换成 `--autoConnect --user-data-dir=<abs>\.webview2-dev-cdp\EBWebView`，让 MCP 自己读 `DevToolsActivePort`。需要 MCP 支持该 flag 且 user-data-dir 写绝对路径。

历史踩坑（已规避）：直接写 `command: "npx"` 在 Windows 上立刻 `spawn ENOENT`，因为 Node 的 `child_process.spawn` 不补 `.cmd` 后缀。换成 `"npx.cmd"` 不会 ENOENT 但仍然慢到超时 — 所以最终落在 `command: "node"`。

### 单次会话流程

1. 终端 1 起 `pnpm tauri:dev:cdp`，等到 Ridge 窗口可见
2. 终端 2 跑 `pnpm cdp:smoke` 验证（**强烈推荐**，省得后面 MCP 报错难定位）
3. 让 Claude 用 `mcp__chrome-devtools-tauri__list_pages` → `take_snapshot` → `evaluate_script` 等工具驱动 Ridge

> MCP 服务器在首次工具调用时启动并尝试连接 9222。如果那时 Tauri 没起来，MCP 会启动失败，需要**重启 Claude Code 会话**才能恢复。务必先 1→2→3 顺序。

### 常用工具速查

| 目的 | 工具 |
| --- | --- |
| 找到 Ridge 主窗口 | `mcp__chrome-devtools-tauri__list_pages` |
| 切到目标 | `mcp__chrome-devtools-tauri__select_page` |
| 拿 DOM accessibility 树 | `mcp__chrome-devtools-tauri__take_snapshot` |
| 跑 JS（含 `window.__TAURI__` 调用） | `mcp__chrome-devtools-tauri__evaluate_script` |
| 点击 / 输入 / 按键 | `click`, `fill`, `type_text`, `press_key` |
| 截图 / 全页 | `take_screenshot` |
| 抓 console / 网络 | `list_console_messages`, `list_network_requests` |
| Perf trace | `performance_start_trace` → 操作 → `performance_stop_trace` |

---

## 4.5 自动断言探针（headless e2e，无需 MCP）

除了 agent 驱动的 MCP 探索，`scripts/cdp-*.mjs` 还有一组**自带断言、退出码即结论**的 node 脚本——CI 友好，也能让 agent 一条命令拿到 PASS/FAIL。它们都走「CDP attach → `invoke(...)` → 后端 → （部分）LAN WS」这条链路，对**真实运行的 Ridge 后端**做端到端验证。**端口经 `resolveCdpPort()` 自动发现（动态端口安全）**，无需手动指定。

| 命令 | 脚本 | 验什么 | 结论 |
| --- | --- | --- | --- |
| `pnpm cdp:smoke` | `cdp-smoke.mjs` | 9222 可达 + 至少一个 Ridge page target | exit 0/1 |
| `pnpm cdp:pty` | `cdp-pty-parsers.mjs` | **`ridge_core::pty` 解析层**：decode(增量 UTF-8 多字节回环)、title(OSC 0/1/2)、cwd(OSC 7) 经 PTY→后端解析→LAN 转发端到端 | exit 0/2 + 三项 PASS/FAIL |
| `pnpm cdp:pane-graph` | `cdp-pane-graph.mjs` | **pane CRUD 特征化**（D11 Wave A 的 P2 gate）：split/close 经 CDP `invoke` → `get_pane_layout` 断言 ±1 leaf（树真相，`ridge_core::workspace::pane_tree` 所有）+ LAN WS 断言 `panes` 广播帧（`PanesChanged` 重枚举路径）。**不**断言 LAN panes 计数（headless split 无 PTY 故不入 `terminals`）；native-detach 抑制不在自动网内 | exit 0/2 + 四项 PASS/FAIL |
| `node scripts/cdp-lan-probe.mjs` | `cdp-lan-probe.mjs` | LAN 线协议（hello/panes/subscribe/二进制帧 UUID 布局/echo 回环） | exit 0/2 |
| `node scripts/cdp-term-input.mjs ["cmd"]` | `cdp-term-input.mjs` | 向可见终端注入一行（默认 emoji 测试表）供截图——**非断言**，配 MCP 截图用 | inject ✓ |
| `node scripts/cdp-teammate-e2e.mjs` | `cdp-teammate-e2e.mjs` | **Domain Zero teammate 后端**（D1/D2）：`classify_command_risk`（L0/L1/L2 + 抗空格绕过 `git   push`）、`get_teammate_topology`、`set_hitl_enabled` 经 live `window.__TAURI__.core.invoke` 端到端 | exit 0/1 + 7 项 PASS/FAIL |

`cdp:pty` 设计要点（写**可重复** e2e 的范式）：
- 注入的是**纯 ASCII 源**的 PowerShell 单行，用 `[char]::ConvertFromUtf32(...)` 在**输出端**生成 3/4 字节码点（∑ 😀 你好 🇯🇵），从而只考验输出 decode 路径而非 stdin 编码；并 `[Console]::OutputEncoding=UTF8` 让 Windows PowerShell 5.1 也吐 UTF-8。
- title 用**每次运行的 nonce**（`Date.now()`）：桌面对**未变化**的 pane 标题会去重（同值不再发 `PaneTitleChanged`），所以固定标题第二次跑会假阴性——必须每次换新标题才幂等。cwd 因 PowerShell prompt 每次重发真实 cwd 而天然不被去重。
- `find_prompt_osc`（prompt OSC）**不经 LAN WS 转发**，故由 `ridge-core` 单测覆盖，不在此 e2e 内。

> 前提：先 `pnpm tauri:dev:cdp` 起调试实例（它与正式版并存、不互杀）。这些脚本会自轮询等待 Ridge target（最长 90s），可在 dev 启动后立刻跑。

---

## 4.6 驱动前端事件 + 可视化验证（teammate Domain Zero 实战）

验证「后端 emit 某事件 → 前端组件 `listen` 渲染」这类链路时，**不必真触发后端**（往往要 teammate URL/token、真 agent 流程，很重）。Tauri v2 事件是全局的：**前端 `emit` 也能触发前端 `listen`**，与后端 emit 同一路径。所以经 CDP `Runtime.evaluate` 调 `window.__TAURI__.event.emit('事件名', payload)`，即可让对应组件渲染、再截图坐实。

### 范式（raw CDP，无需 MCP）

```js
import { resolveCdpPort } from './cdp-port.mjs';
// 1) /json/list 找 Ridge page → new WebSocket(t.webSocketDebuggerUrl)（Node 22+ 自带 WebSocket）
// 2) Runtime.enable
// 3) 调命令：await Runtime.evaluate(`window.__TAURI__.core.invoke('set_hitl_enabled', {enabled:true})`, {awaitPromise:true})
// 4) emit 与后端完全相同 shape 的事件 payload：
//    window.__TAURI__.event.emit('teammate://hitl-approval-required', {id,initiator,action,level:'Dangerous',reason})
// 5) Runtime.evaluate 读 document.body.textContent 断言渲染；Page.captureScreenshot 截图
```

实测过的三条 teammate UI（payload shape 必须与后端 emit 完全一致）：

| 域 | 事件 | payload（与后端一致） | 前端效果 |
| --- | --- | --- | --- |
| D2 HITL 模态 | `teammate://hitl-approval-required` | `{id,initiator,action,level:'Dangerous',reason}`（`hitl.rs`） | `HitlApprovalModal` 居中弹出（L2 徽章/命令/三按钮/队列 +N） |
| A3 TML 审计 | `teammate://tml-message` | `{header:{from_pane,to_pane,action:{type,payload}},body}`（`TmlMessage` serde） | Agent Center「协作审计」刷出人话化行 |
| D3 熔断 | `teammate://circuit-tripped` | `{workspaceId,paneId,reason}`（`circuit.rs`） | Agent Center 置顶红色「熔断告警」 |

> 改了 `.svelte`/`.ts` 后 **vite HMR 会热重载**运行中的 dev 应用（无需 rebuild Rust），前端改完可立刻 emit 测——但 HMR 会重挂组件、**清掉组件内存态**（如审计列表），重测前重新 emit 即可。

### 截图技巧 / 坑

- **WebGPU 画布截不到**：终端区域是 WebGPU 宿主画布，`Page.captureScreenshot` 抓回**空白**——已知现象，**非应用故障**（`window.__TAURI__` 在、`invoke` 成功即证明应用健康）。**DOM 区域（侧栏/模态/插件面板）能正常截**。
- **精确裁剪 DOM 区域**：`Page.captureScreenshot({ clip:{x,y,width,height,scale:2} })`。元素 box 拿不到（Svelte 包裹层常 0×0）时，用 `el.scrollIntoView()` + 按 `window.innerHeight` 算固定区域（如侧栏页脚 `y: vh-230`）兜底。
- **全局侧栏插件需侧栏展开才渲染**：Agent Center 注册为 `scope:'global'` 侧栏插件；全新 profile 启动、侧栏没开时 `innerText` 里查不到它——先确认 dev 应用有打开的工作区/侧栏。
- **模态遮罩污染背景截图**：HITL 模态是全屏 `bg-black/60 backdrop-blur` 覆盖层，**没关掉它时**其它区域截图会发暗发糊。截别的前先点「拒绝」清场（`[role=alertdialog]` 里找含「拒绝」的 button `.click()`）。

---

## 5. 故障排除

| 症状 | 原因 | 处置 |
| --- | --- | --- |
| **CDP 端口死活连不上、无 `DevToolsActivePort`，但 Ridge 窗口已开** | **Chromium 136+ 拒绝固定 `--remote-debugging-port`**（见顶部 banner） | 确认在用**新版** `tauri-dev-cdp.mjs`（`--remote-debugging-port=0`）；端口看 `cat .webview2-dev-cdp/cdp-port.txt` 或日志 `✅ CDP ready on port N`。**别再设固定 `CDP_PORT`** |
| Tauri 启动直接 `HRESULT 0x8007139F` | 已安装的 Ridge 在跑、共用 EBWebView 目录 | 关掉正式版 Ridge，或确认 `.webview2-dev-cdp/` 被使用 |
| `cdp:smoke` 超时 / connection refused | Tauri 还没起完 / 端口未发现 | 等 Ridge 窗口出现 + 日志打出 `CDP ready on port N`；`resolveCdpPort()` 会自动读 `DevToolsActivePort` |
| `cdp:smoke` 提示 "no obvious Ridge target" | URL scheme 变了 | 看脚本打出的 targets 列表，确认主页路径，必要时更新 `cdp-smoke.mjs` 里的匹配规则 |
| MCP 工具调用报 `ECONNREFUSED 9222` | MCP 写死 9222，但端口是动态的（或 MCP 在 Tauri 起来前 cold-start） | 见 §4「动态端口下 MCP 连不上」——首选直接用 raw-CDP 脚本；或把 `--browserUrl` 改成真实端口后重启会话 |
| MCP 健康检查报 `spawn npx ENOENT` | Windows 上 `command: "npx"` 不解析 `.cmd` | 直接换成 `command: "node"` + 绝对路径（见 §4 配置块）；不要再用 `npx`/`npx.cmd` |
| `chrome-devtools-tauri` 一直 "still connecting"，过几秒后消失，`ToolSearch` 找不到 schema | `npx pkg@latest` 启动 ~19s，超 MCP host startup window | 换成 `node + 全局 bin 绝对路径`（实测 ~5s），重启 Claude Code 会话 |
| `take_snapshot` 看到的页面是空白 | 选到了非 Ridge target（如 DevTools UI） | 先 `list_pages`，用 `select_page` 切到 title 为 `Ridge` 的 page 目标（dev: `http://127.0.0.1:5173/`；prod: `tauri://localhost/`） |
| ToolSearch 找不到 `mcp__chrome-devtools-tauri__*` schema | MCP server 完全断开（不只本条目，可能整批 MCP 一起死） | 退出并重启 Claude Code 会话，Tauri 端不用动；重启后 `pnpm cdp:smoke` 仍能验证链路 |
| `pnpm tauri:dev:cdp` 后台跑没多久就没了 | 用 `... &`（Git Bash）后台启会被 SIGHUP 杀（无 `disown`） | 用 harness 的 `run_in_background:true` 跑，或 `nohup ... &`；别用裸 `&` |
| 想干净杀掉 dev:cdp（重启换新代码） | 进程树是 `node tauri-dev-cdp.mjs → pnpm tauri dev → cargo run → ridge.exe → msedgewebview2` | `Get-CimInstance Win32_Process -Filter "Name='node.exe'" \| ? CommandLine -match 'tauri' \| % { taskkill /F /T /PID $_.ProcessId }` + `Stop-Process cargo,rustc -Force`。**别杀 `msedgewebview2`**（分不清装机版与 dev 的，要杀只按 `--user-data-dir~webview2-dev-cdp` 过滤），**别杀装机版 `ridge.exe`**（托管本会话） |
| `ridge` crate 编译卡在 628/629 很久 | 单 crate 链接慢（非 churn）；或并发会话频繁改 `src-tauri` 触发 watcher 反复重建 | 查 `grep -c "Rebuilding" 日志` + `find src-tauri/src -name '*.rs' -mmin -10`；前者 0 + 后者空 = 只是链接慢，等即可 |

---

## 6. 与其它测试栈的关系

- **vitest** (`pnpm test`)：纯逻辑单测，不接 CDP。
- **wdio E2E** (`pnpm e2e:shell`, `e2e:perf`, `e2e:reset`)：黑盒、msedgedriver、有 spec 文件、跑回归。**保留**。
- **chrome-devtools-tauri MCP**：白盒探索 + bug 复现 + 性能截取，由 agent 调度，不写死在 spec 文件里。三者职责互补，不冲突。

---

## 7. 宿主回滚

如果想撤销 MCP 配置：

```pwsh
Copy-Item $env:USERPROFILE\.claude.json.backup-2026-05-22 $env:USERPROFILE\.claude.json -Force
```

或手动从 `~/.claude.json` 的 `mcpServers` 里删掉 `chrome-devtools-tauri` 即可，**原 `chrome-devtools` 条目未被修改**。
