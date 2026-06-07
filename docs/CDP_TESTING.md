# Ridge CDP 测试工作流

借助 `tauri:dev:cdp` + `chrome-devtools-tauri` MCP，把**真实 Tauri WebView2 运行时**暴露给自动化工具，从而对 Ridge 做端到端 UI / IME / 终端 PTY / 性能验证 —— 而不是去测一个没有 `window.__TAURI__` 的纯 vite 页面。

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

- 默认 CDP 端口 `9222`（可 `CDP_PORT=9333 pnpm tauri:dev:cdp` 覆盖）
- 通过 `WEBVIEW2_USER_DATA_FOLDER` 指向项目根的 `.webview2-dev-cdp/`，**与已安装的 Ridge 完全隔离**
- 否则 dev 和正式版共用 `%LOCALAPPDATA%\com.<bundleId>\EBWebView` 会触发 HRESULT `0x8007139F`（ERROR_INVALID_STATE）

`.webview2-dev-cdp/` 已加入 `.gitignore`。

---

## 3. 启动顺序（关键）

chrome-devtools MCP 在 `--browserUrl` 模式下**冷启动时**就会尝试连 9222。Tauri 必须先起来。

```pwsh
# 终端 1 — 启动带 CDP 的 Tauri
pnpm tauri:dev:cdp

# 等输出形如:
#   [tauri-dev-cdp] WebView2 CDP   : http://127.0.0.1:9222
# 并且 Ridge 主窗口已经显示

# 终端 2 — 探活
pnpm cdp:smoke
```

`pnpm cdp:smoke` 期望输出：

```
[cdp-smoke] connected to 127.0.0.1:9222
[cdp-smoke] browser       : Edg/148.0.3967.70
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

除了 agent 驱动的 MCP 探索，`scripts/cdp-*.mjs` 还有一组**自带断言、退出码即结论**的 node 脚本——CI 友好，也能让 agent 一条命令拿到 PASS/FAIL。它们都走「CDP attach → `invoke('set_remote_enabled')` → 轮询 `get_remote_info` → LAN WS」这条链路，对**真实运行的 Ridge 后端**做端到端验证。

| 命令 | 脚本 | 验什么 | 结论 |
| --- | --- | --- | --- |
| `pnpm cdp:smoke` | `cdp-smoke.mjs` | 9222 可达 + 至少一个 Ridge page target | exit 0/1 |
| `pnpm cdp:pty` | `cdp-pty-parsers.mjs` | **`ridge_core::pty` 解析层**：decode(增量 UTF-8 多字节回环)、title(OSC 0/1/2)、cwd(OSC 7) 经 PTY→后端解析→LAN 转发端到端 | exit 0/2 + 三项 PASS/FAIL |
| `node scripts/cdp-lan-probe.mjs` | `cdp-lan-probe.mjs` | LAN 线协议（hello/panes/subscribe/二进制帧 UUID 布局/echo 回环） | exit 0/2 |
| `node scripts/cdp-term-input.mjs ["cmd"]` | `cdp-term-input.mjs` | 向可见终端注入一行（默认 emoji 测试表）供截图——**非断言**，配 MCP 截图用 | inject ✓ |

`cdp:pty` 设计要点（写**可重复** e2e 的范式）：
- 注入的是**纯 ASCII 源**的 PowerShell 单行，用 `[char]::ConvertFromUtf32(...)` 在**输出端**生成 3/4 字节码点（∑ 😀 你好 🇯🇵），从而只考验输出 decode 路径而非 stdin 编码；并 `[Console]::OutputEncoding=UTF8` 让 Windows PowerShell 5.1 也吐 UTF-8。
- title 用**每次运行的 nonce**（`Date.now()`）：桌面对**未变化**的 pane 标题会去重（同值不再发 `PaneTitleChanged`），所以固定标题第二次跑会假阴性——必须每次换新标题才幂等。cwd 因 PowerShell prompt 每次重发真实 cwd 而天然不被去重。
- `find_prompt_osc`（prompt OSC）**不经 LAN WS 转发**，故由 `ridge-core` 单测覆盖，不在此 e2e 内。

> 前提：先 `pnpm tauri:dev:cdp` 起调试实例（它与正式版并存、不互杀）。这些脚本会自轮询等待 Ridge target（最长 90s），可在 dev 启动后立刻跑。

---

## 5. 故障排除

| 症状 | 原因 | 处置 |
| --- | --- | --- |
| Tauri 启动直接 `HRESULT 0x8007139F` | 已安装的 Ridge 在跑、共用 EBWebView 目录 | 关掉正式版 Ridge，或确认 `.webview2-dev-cdp/` 被使用 |
| `cdp:smoke` 超时 / connection refused | Tauri 还没起完 / 端口被别的进程占 | 等 Ridge 窗口出现；`Get-NetTCPConnection -LocalPort 9222` 看占用 |
| `cdp:smoke` 提示 "no obvious Ridge target" | URL scheme 变了 | 看脚本打出的 targets 列表，确认主页路径，必要时更新 `cdp-smoke.mjs` 里的匹配规则 |
| MCP 工具调用报 `ECONNREFUSED 9222` | MCP 在 Tauri 起来前 cold-start | 关掉 Tauri → 退出 Claude → 重启会话 → 起 Tauri → 起会话 |
| MCP 健康检查报 `spawn npx ENOENT` | Windows 上 `command: "npx"` 不解析 `.cmd` | 直接换成 `command: "node"` + 绝对路径（见 §4 配置块）；不要再用 `npx`/`npx.cmd` |
| `chrome-devtools-tauri` 一直 "still connecting"，过几秒后消失，`ToolSearch` 找不到 schema | `npx pkg@latest` 启动 ~19s，超 MCP host startup window | 换成 `node + 全局 bin 绝对路径`（实测 ~5s），重启 Claude Code 会话 |
| `take_snapshot` 看到的页面是空白 | 选到了非 Ridge target（如 DevTools UI） | 先 `list_pages`，用 `select_page` 切到 title 为 `Ridge` 的 page 目标（dev: `http://127.0.0.1:5173/`；prod: `tauri://localhost/`） |
| ToolSearch 找不到 `mcp__chrome-devtools-tauri__*` schema | MCP server 完全断开（不只本条目，可能整批 MCP 一起死） | 退出并重启 Claude Code 会话，Tauri 端不用动；重启后 `pnpm cdp:smoke` 仍能验证链路 |
| 想用别的端口 | 默认 9222 冲突 | `$env:CDP_PORT="9333"; pnpm tauri:dev:cdp`；同时改 MCP `--browserUrl` 或临时 `CDP_PORT=9333 pnpm cdp:smoke` |

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
