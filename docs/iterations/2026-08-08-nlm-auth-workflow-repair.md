# NLM 认证与迭代工作流修补记录（2026-08-08）

## 结论

NotebookLM 认证已恢复，且已验证 CLI/MCP 可用。根因不在 Google 账号本身：首次启动外部 Chrome 后，流程没有等待 CDP 就绪；同时 `notebooklm-mcp-cli` 对 `notebook.google.com` 的登录判定与实际落点不一致，使 `nlm login --cdp-url` 可能在浏览器已登录后继续等待。

本轮修补只涉及工作流、认证 helper、文档与本地生成物忽略规则，不改 Ridge 产品代码。

## 刚刚的尝试与证据

1. 直接运行 `nlm login --check` 时，CDP/代理链尚未稳定，失败结果不能单独证明账号未登录。
2. 使用固定代理 `http://127.0.0.1:51081` 启动专用 Chrome；页面落点为 `https://notebook.google.com/`，浏览器版本为 `Chrome/150.0.7871.187`。
3. 用户完成登录后，使用 `save_external_cdp_auth.py` 从外部 CDP 保存认证：`saved=true`、`cookie_count=40`、`csrf_present=true`、`session_present=true`。未读取或输出 Cookie 值。
4. 使用同一代理验证：`login_check_exit=0`、`notebook_list_exit=0`、`notebook_count=22`。
5. NotebookLM MCP 已能访问目标笔记本；`notebook_get` 显示当前仍有 3 份来源：`Agent 通信架构重构`、`PROJECT-STATE.md`、`REQUIREMENTS-SPEC.md`。故单一来源不变量尚未闭合；本轮未执行不可逆的 `source_delete`。
6. 最近对话结论仍只作规划输入，代码结构与完成判定以 CodeGraph、源码、测试及运行证据为准。

## 已固化的工作流护栏

- `nlm_auth_flow.py launch` 启动后等待 `/json/version` 与 `/json/list`，并显式返回 `cdp_ready`、浏览器与页面元数据。
- 认证流程拆分五类故障：CDP 未就绪、浏览器未登录、认证抽取失败、CLI 网络/包装器失败、NotebookLM API 失败。
- `nlm login --check` 只作预检；用户明确要求刷新时不得因预检失败跳过刷新。
- 抽取阶段使用 `notebooklm-mcp-cli` 的 Python 环境和 `wait_for_login=False` 路径，不依赖会产生假性等待的 `nlm login --cdp-url`。
- CLI 与 Chrome 共用固定代理；专用 Chrome profile；Cookie、storage、密码、token 不进入输出、日志、NotebookLM 来源或 Git。
- 仅在登录检查与 `notebook list` 均成功后进入冷循环；失败先留分类诊断，不阻塞本地代码验证。
- `.tools/` 与 `.scannerwork/` 纳入忽略，避免本机 JDK/Sonar 运行态污染迭代输入。

## 冷循环入口

每轮仍遵循：CodeGraph 更新并取代码事实 → 覆盖更新唯一 `PROJECT-STATE.md` → 替换 NotebookLM 唯一常驻来源 → 查询下一轮合同 → 独立对抗评审 → 本地归档。NotebookLM 只给规划与根因假设，不作为代码完成证明。

本次认证证据足以开启下一轮 NotebookLM 查询；但在清理 3 份来源前，冷循环只可做只读诊断。未宣称 Remote 的物理手机、公网 WebRTC、WebView2、双窗口或 Sonar Quality Gate 已闭合。

## 认证后再次尝试与对话回流（2026-08-08）

- MCP 首次读取时再次返回 `Authentication expired`；`refresh_auth` 只能确认磁盘凭据已过期，不能伪造成功。
- 按外部 CDP 流程重新抽取并保存后，MCP `refresh_auth` 返回 `success`，随后 `chat_list/chat_get` 可读。
- CLI `login --check` 曾在输出“Authentication valid、22 notebooks”后因包装器未及时退出而超时；故记录为“业务检查成功、进程退出异常”，不得只看超时码判失败。
- 近期可读对话的真实痛点包括：模型长期停在查代码/列清单并触发 `max stall`、历史思考与工具输出难以直接滚动回看、输入框提示噪声、Remote/Explorer/PTY/render 连续性、移动端 geometry/DPR，以及 Jules/质量遥测/搜索路由的工作流隔离。
- 对话中还出现“REQ 已 Active、报告已生成、能力已落地”等超出本地证据的断言；与本地需求文件不一致者均降级为 `out_of_date_or_unverified`，不据此改 Active 条款。

## 已补的 NLM 工作流闸门

`notebooklm-iteration-loop/SKILL.md` 与模板现明确：NLM 默认只读；`chat_list/chat_get` 只作痛点输入；来源、notes、Studio、分享等写操作须用户逐项授权；NLM 自称“Approved/已完成”不构成授权或验收；发布、Remote/Cloud 激活、`git push` 均停在 `awaiting_user_authorization`。本轮仍未做任何外部发布或 NotebookLM 来源/笔记写操作。
