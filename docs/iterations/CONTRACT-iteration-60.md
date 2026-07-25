# CONTRACT — iteration 60

- **日期**: 2026-07-26
- **需求源**: 用户↔NLM 今日对话原文（`2026-07-26-nlm-conversations-today.md`，10 问全量抓取）；对抗评审放宽为「不驳回、附替代案问用户」，三项决策已由用户裁定（品牌层改名 / 发现+编组工具 MVP / 搭 R-TESTGATE）。
- **评审核实记录**: NLM「resize 被能力门拦截」证伪（`resize_pane` 在 REMOTE_ALLOWLIST，remoteAllowlist.ts:54）；`preferWebgpu` 默认已 true（manager.ts:643）；`$/cancel` 只短路响应不杀进程；process_guard 有杀树+超时、无 per-key 取代。

## 目标（优先级）

| ID | P | 事 | 确定性验收 |
| --- | --- | --- | --- |
| G1 GIT-ABORT | P0 | git 检索 per-key（repo/kind）latest-win：同键新请求即杀旧进程树+释放许可；cwd 快速切换不再堆积等超时 | ridge-core 单测：真挂起假二进制被取代杀掉、许可即时释放、活跃数≤cap；`cargo test -p ridge-core` 绿 |
| G2 DESKTOP-FILL | P0 | 桌面浏览器分屏终端：shell 渲染区填满 pane 内容区；「re-claim size」按钮生效 | 根因写入迭代报告；fit/measure 纯逻辑测；CDP 截图证据（分屏拖拽后字符区跟随、无大空框） |
| G3 MOBILE-SIZE | P0 | 手机 remote 终端尺寸与视口一致（0.1.1 P4 保活回归）：attach/unpark/resume 后强制按容器 rect 重测并下发 resize | 尺寸重发路径纯逻辑测；CDP 移动视口证据（rows/cols 随视口变化） |
| G4 WEBGPU-FIRST | P1 | 远端 remote 恢复 WebGPU 优先；回退时输出探测失败原因（不再静默） | 诊断结论入报告；backend 选择路径断言测；CDP 证据 backendName=webgpu（支持环境）或显式回退日志 |
| G5 COMMUNE-BRAND | P1 | 品牌层改名「Agent's Commune」：UI 标签/内置 MCP 对外名/文档；wire 方法名不动 | grep UI 无旧「Agent Center」文案残留（代码符号除外）；svelte-check 绿 |
| G6 DISCOVERY-MVP | P1 | 轻量 Agent 自动发现：进程指纹+单层 cwd 嗅探，事件驱动+防抖，设置开关；入 roster「Discovered」 | 指纹匹配+拓扑注入单测；无递归扫描（代码断言）；关开关即停 |
| G7 COMMUNE-TOOLS | P1 | Commune MCP 只读编组工具：agent 可查队友 roster/编组/summary | teammate server 测试新增该 tool 断言 |
| G8 R-TESTGATE | P2 | CI 前置 test job：vitest + svelte-check + 关键 vite build 冒烟，红则阻发布 | release.yml（或前置 CI）含该 job 且 needs 链生效 |
| G9 PANE-META-BCAST | P0 | （用户 7-26 追加）Remote 头部标题+CWD、Pane 选择弹层信息随 pane 信息广播实时刷新；广播机制缺失则先建（host 推送 pane title/cwd 变更事件 → 控制端订阅刷新） | 控制端纯逻辑测：收到 pane-meta 事件后 header/弹层数据源更新；host 侧事件发射单测或既有事件路径证据 |
| G10 MOBILE-NAV-BACK | P1 | （用户 7-26 追加）手机 Remote 打开文件关闭后返回上一级页面，而非直接回终端主页 | 导航栈纯逻辑测：open→close 回到前一视图；svelte-check 绿 |
| G11 IME-DEDUP | P1 | （用户 7-26 追加）输入法自动补全重复：已输 `Spac` 选补全 `Space` 时实际发出 `SpacSpace`。建「已发送段标记 + 补全去重公共前缀」机制，只补差量（含必要退格） | 纯函数测：`sent="Spac"`+`insert="Space"` → 增量 `e`；含中途退格/全角/多词用例 |

## 顺手项（性能/流畅度授权内）
- codegraph 索引剔除 `.venv-notebooklm/`、`.pnpm-store/` 等污染目录（索引 1495 文件中含 pip vendor）。
- 实现途中遇到的明显卡顿/低效路径按 Ponytail 顺手修，报告中记一行。

## 边界（不做）
- 不深改协议方法名（get_teammate_topology 等 wire 名保持；品牌层完成后即收）。
- 不做 R-INCR / R-WSLEG / R-P4-LRU / R-P5P6（留 backlog）。
- 不引入 CRDT/pairing/daemon；不做云出站二期外新出站形态。

## 停机条件
- 同一目标连续 2 次验收不过 → 停下报告；G2/G3 若 CDP 环境不可得，以纯逻辑测+代码证据交付并明记「真机证据缺」。

## 收尾
- 迭代报告 `2026-07-26-iteration-60.md`；ARCHITECTURE.md 增量回写；NLM 来源替换（659ffc20 → 新版）。
- 发布：v0.1.3 三处版本号同步 → tag 触发 release.yml 等矩阵资产 → `publish:remote-cloud`。
