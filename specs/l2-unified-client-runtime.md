---
id: L2-UNIFIED-CLIENT-RUNTIME
level: L2
title: Desktop presentation client and unified CLI/Remote runtime
status: DRAFT
lifecycle: PLANNED
parent: L1-PROJECT-001
code_targets:
  - Cargo.toml
  - Cargo.lock
  - packages/ridge-core/**
  - packages/ridge-kernel/**
  - packages/ridge-cli/**
  - packages/ridge-remote/**
  - packages/remote/**
  - packages/ridge-acp/**
  - packages/ridge-mcp/**
  - packages/ridge-term/**
  - src-tauri/src/**
  - src-tauri/Cargo.toml
  - src-tauri/tauri.conf.json
  - src-tauri/capabilities/**
  - src/lib/**
  - src/routes/**
  - src/remote/**
  - vite.config.js
  - scripts/**
  - docs/archive/**
test_targets:
  - packages/ridge-core/**
  - packages/ridge-kernel/**
  - packages/ridge-cli/**
  - packages/ridge-remote/**
  - packages/remote/**
  - packages/ridge-acp/**
  - src/lib/transport/tauriShim/bridge.test.ts
depends_on:
  - L2-AGENT-COMMUNE-EXACT-BINDING
  - L2-PROTO-001
  - L2-REMOTE-001
  - L2-RIDGE-COMMUNION-M53
  - L2-RIDGE-PRODUCT-M1
  - L2-TERM-001
  - L3-REMOTE-INTERACTION-RELIABILITY
  - L3-TERMINAL-VISUAL-INPUT-REPAIR
---

# 桌面展示层化：与 CLI、Remote 共用一条业务链路

## 状态与依据

2026-10-11 用户已将 Commune 与 A2A 方案提升为确定实施内容；其批准记录与验收边界见 `CHG-070.commune.md`。本次定版未改 CHG-069 的权威图、批准或锁。

用户已明确要求：桌面绕过 Tauri 业务后端，全面与 CLI 保持同一链路，桌面仅为展示层；本轮要求交付改动方案并排入下一次 change。此目标已明确，以下迁移机制为设计提案，尚未执行、尚未批准实施锁。

2026-10-11 用户进一步明确：只保留左侧 Agent’s Commune；移除右侧 Communion；Agent 由用户在终端运行，Ridge 经 ACP 等通用协议登记同一既有会话；禁止按品牌业务适配、自动启动 Agent或另建会话冒充接管；支持用户清单扩展和协议兼容的未知 Agent。成员/编组/历史及 A2A 设计详见同目录 `CHG-070.commune.md`，配套节点 `L3-COMMUNE-PROTOCOL-MEMBERSHIP` 的正文由该文件生成。

候选号 CHG-070。CHG-069 正在持锁实施；本文件与同名 JSON 均为 `.spectree/proposals/` 候选，不写入当前权威图、不修改 CHG-069 的批准或锁。CHG-069 完成后重新执行 `stc next`，通过 `stc change` 登记当时实际下一个编号；若编号已占用，整体重编号，再经提案应用、review、approve、lock、build、verify、complete。不得直接复制本候选覆盖 `changes/`。当前工具没有 change 正文导入接口；后续需通过受支持的 SpecTree 正文写入能力登记本方案，不以手改权威文件补工具缺口。

## 0. 与 CHG-069 的需求边界

| 需求 | 主责 change | 另一 change 的责任 |
| --- | --- | --- |
| 长按光标逐帧呈现、链接识别与 GPU 下划线 | 069 | 070 接入新链路后回归，不重写语义 |
| 键盘/IME/鼠标输入、客户端 FIFO/背压、切换与重连恢复 | 069 | 070 将同一客户端/handler 接至统一 Host，保留顺序与代际契约 |
| 手机后台切回重连、有效会话刷新免验证码 | 069 | 070 迁服务归属并回归；不另改认证期限/绑定 |
| 语义行历史加载与有界客户端缓存 | 069 | 070 迁移服务入口/运行所有者，复用 069 行分页实现与 schema |
| WASM 功能拆包、按需加载、冷启动首屏性能 | 069 | 070 打包/服务分发兼容这些模块；独立 Runtime 打包另属 070 |
| Remote 默认简约 UI；仅 ui=desktop 切桌面 | 069 | 070 同时支持两类浏览器客户端；原生桌面展示层化不改变其 UI |
| Tauri 业务 dispatcher/AppState/持久化/后台 owner 迁出 | 070 | 069 不承担迁移，不新增这类权威 |
| 桌面/CLI/Remote 同一 Host 业务与事件链路、能力对等 | 070 | 069 将终端逻辑做成可复用实现与接口，避免绑定新的桌面业务服务 |
| 独立 Runtime 启停/发现/打包、退出桌面后服务存活、数据迁移 | 070 | 069 保持现有业务生命周期，不承诺此验收 |

两项可触及相同源文件，不能据路径相交认定需求重复。069 对 Host/parser 的修改限必要适配与共享终端实现；不得另建桌面权威或复制多套行分页、认证、分发。070 负责搬移或替换连接/运行归属，终端契约引用 069 的权威节点；不得为服务迁移重新设计 cursor/link、历史协议、WASM 初始化或 UI 路由。

先完成 069，再按其交付源码与实际证据重算 070 影响及 allowedPaths。070 必须把继承的终端契约验收与自己新增的业务归属/进程生命周期验收分开计量；后者不可借 069 测试冒充。069 未完成项不得悄然转移为 070 任务，亦不得把 070 业务迁移提前塞入 069。必要临时适配须只调用共享实现，070 退役时归档旧适配，不保留业务 fallback。

## 1. 目标链路

```text
桌面 WebView ─┐
CLI / TUI ────┼─→ 同一 Host 接入与业务分发 → 同一无 Tauri 业务服务 → Ridge Kernel
Remote 浏览器 ┘       本地 / LAN / Cloud 传输适配                 PTY / 运行态

Tauri 壳：窗口与系统展示交互、受限启动/发现入口
```

“同一链路”指同一命令语义、业务实现、权限判定、状态权威与事件来源。不同接入可有传输适配和既有权限差异，不能维护另一份桌面 dispatcher、另一套业务状态或另一份输出解析。浏览器无须暴露或直连 Kernel 私有端口；Host 仍负责认证、能力授权、连接管理与协议适配。

最终桌面业务命令不经 `tauri::command`，业务事件不依赖 Tauri `emit`/`Channel`，Remote 不再进入 `DesktopHost → commands::*`。CLI LAN、CLI Cloud、CLI/TUI 及桌面都使用统一服务；不能仅把桌面改为调用当前 `ridge host` 的有限命令表便宣告完成。

CLI/TUI 的领域操作亦走该 Host 的共同请求边界，不能各自持有服务状态或直接读写业务数据库。进程启动、诊断等管理入口可独立存在，但不能成为第二个业务执行入口。同一用户与已认证宿主身份下，桌面和 CLI 并发启动须经现有 endpoint/registry 与互斥发现机制复用同一权威实例；不能重复拉起各自的服务副本。多 Host 是连接不同已认证宿主，不是同一宿主多份业务权威。

## 2. 当前缺口与复用依据

| 已核对入口 | 现状与改动方向 |
| --- | --- |
| `packages/ridge-cli/src/host.rs:81` | CLI LAN 使用 KernelHost，复用其独立 Host 模式 |
| `packages/ridge-cli/src/kernel_host_impl.rs:1206` | CLI 自有分发；部分产品命令明确不支持，需补齐共同业务服务 |
| `packages/ridge-cli/src/daemon.rs:204`、`rpc.rs`、`tui/lan_host_impl.rs` | Cloud/TUI 仍有独立入口，纳入同一能力与事件契约 |
| `src-tauri/src/remote_bridge.rs:442`、`remote_host_impl.rs:3113` | DesktopHost 与终端 commands 转发需退役归档 |
| `src-tauri/src/lib.rs:334`、`state.rs` | 大量业务命令和 AppState、后台服务驻留桌面进程，全面迁出 |
| `packages/ridge-core/src/dispatch.rs` | 已有无 Tauri dispatch、能力校验与业务实现，优先复用、补齐 |
| `packages/ridge-kernel/src/domain.rs`、`client.rs`、`rtp1_session.rs` | 已有独立 Kernel 与协议，保留权威与运行代际语义 |
| `src/lib/transport/tauriShim/bridge.ts:1` | 桌面 UI 已有浏览器 RPC 适配基础，改为显式业务 client；逐步移除业务 Tauri shim |

CodeGraph 索引不可用，本方案依据以上真实源码；未声称当前全部服务已迁入 Kernel，也未声称 CLI 与桌面能力已有完整对等。

## 3. 职责与权威

| 职责 | 最终所有者 | 桌面职责 |
| --- | --- | --- |
| PTY 创建/写入/resize/销毁、输出序列、snapshot/replay、运行代际 | Ridge Kernel；复用已批准终端/RTP1 契约 | 输入捕获、展示与控制请求；可保留有界渲染镜像 |
| workspace、pane 拓扑、Host/session 关联、恢复记录 | 独立统一 Runtime；现有 Kernel 权威由 Kernel 承接 | 订阅投影；window/client 选择各自隔离 |
| Agent’s Commune、ACP/A2A、teammate/Hub、审批、历史 | 同一无 Tauri 协议/产品服务与现有持久化权威；连接用户既有 Agent | 成员/编组/历史投影；不启动 Agent、不持有进程或审批真相 |
| 文件/搜索/watch、Git、项目历史、设置/主题、字体资源 | 统一业务服务，复用 ridge-core 及现有独立 crate | 展示、编辑与用户交互；保存经授权业务 API |
| 认证、信任、远控开关、Cloud/Host 连接与重连 | 独立统一 Host 服务 | 显示状态、发起显式用户操作 |
| 窗口、托盘、通知、原生对话框、系统剪贴板、系统打开 | 最小 OS 展示适配 | 可保留受限 Tauri API；无领域调度或任意文件/进程执行 |

统一 Runtime 为职责边界，不要求把全部业务塞入 PTY crate。保留现有 Kernel/core/ACP/MCP 的依赖隔离；独立服务通过组合这些 crate 提供能力。不得新增与 Kernel 重复的 workspace/PTY 权威，不得把桌面 AppState 原封不动搬出再保留两份。包与类划分由实施者根据真实代码决定。

本地渲染镜像、query cache、焦点、选区、滚动位置属于有界展示状态；业务实体、审批、恢复、持久化修改属于服务。客户端关闭仅 detach，不自动终止 PTY、Agent、Host 或其他客户端。

桌面安装包须携带或明确发现与前端兼容的独立运行程序；无需用户另装 CLI 才可启动。复用现有 Kernel 生命周期与版本发现机制，打包/升级也须遵守身份与兼容检查。Windows 下独立 Runtime 不能受 WebView/Tauri 退出时的进程树或 Job 自动销毁；其他平台同样验真实退出后的存活。业务服务停止只接受已有权限下的显式服务管理操作，窗口关闭不等于停止服务。

## 4. 七个实施阶段；同一 change 的完成条件

1. **能力清单与契约收敛。** 枚举 `generate_handler!`、Remote dispatcher、CLI LAN/Cloud/TUI、前端 invoke/listen 调用；逐条归类业务、OS 展示、退役。对每个命令记录服务归属、权限、事件、持久化和验收场景。以实际 UI 能力建立对等清单，覆盖多窗口、Hosts、文件、Git、Agent、审批及恢复，不能只测终端。
2. **统一业务服务。** 将桌面命令实现、后台任务和权威状态迁至无 Tauri 服务。复用 ridge-core 能力校验及现有产品 crate；核心函数不接受 AppHandle/State、不调用 tauri runtime、emit 或 Channel。Desktop 与 CLI 调用同一 handler，不复制代码。
3. **统一 Host 与事件。** 从现有独立 CLI Host 提取可复用入口，LAN/Cloud/TUI 使用共同分发与业务事件源。本地桌面使用同一 RPC/订阅 client，输入、控制、输出保持现有有界队列、序列与取消约束；传输适配不得包含另一套业务分支。
4. **桌面接入。** 桌面启动后发现或受限拉起独立 Runtime，协商版本与能力，完成授权，订阅快照+事件；业务调用不再走 Tauri IPC。复用浏览器桌面入口和共享 remote client。独立 Host 不应依赖 WebView 活跃、前台、未最小化或桌面进程生存。
5. **数据与进程生命周期迁移。** 以现有存储路径、schema、身份和事件流为基础，明确每份桌面旧状态的新唯一所有者。备份/归档、版本标记与可恢复迁移由服务执行；旧新进程禁止双写。启动冲突、权限/证书失败、版本不匹配、部分迁移失败均显式报错，禁止回退桌面本地 PTY/业务执行。进程发现校验身份和运行代际，不能通过端口或进程名猜宿主。
6. **退役旧链路。** 全部对等验收成立后，归档 DesktopHost、Tauri 业务 dispatcher、业务 AppState、PTY/parser/output 转发、桌面业务后台任务以及业务 Tauri shim。保留经清单审计的 OS 展示适配；构建约束禁止现役代码重新引用归档实现。迁移期间兼容桥仅调用共同 handler；最终不得保留回退旧业务链路。
7. **验收并关单。** 所有阶段通过才 complete；中途迁出终端或接通独立 Host 均不等于桌面展示层化完成。CHG-069 已批准的输入、cursor/link、语义行加载、WASM 按需加载与 Remote 默认 UI 行为须保留，迁移不能抵销其契约。

## 5. 安全与多客户端边界

- Kernel 私有凭据仍留宿主进程。WebView 获得限定身份、能力与期限的 client 授权；不通过 URL、日志或 localStorage 暴露 Kernel token。localhost 连接也必须认证、校验 Origin/连接身份、限制请求与消息大小；不为方便桌面接入开放无认证 localhost RPC 或公网 Kernel 端口。
- 本地可信桌面、LAN、Cloud 原有权限边界继续由同一服务依据认证身份判定；共享 handler 不代表 Remote 获得本地特权。不通过 UI 隐藏按钮实现权限隔离。
- 复用已批准的 host/runtime_epoch/workspace/pane/activation、输入幂等、snapshot/delta/replay 与审批绑定契约，不另造并行身份或第二套 wire schema。协议新增字段须在其权威 spec 提案中集中定义并通过版本协商。
- 选择窗口/客户端 workspace 不得改变其他客户端的活动目标；权限、路径沙箱、取消、审批、revision、原子存储校验都在可信服务执行。授权失败、未知方法、失效代际、跨 workspace 请求、重连重复输入均拒绝或按既有幂等契约处理。
- OS 文件选择只返回展示交互结果；文件读写、剪贴板图像保存与进程执行等业务副作用走服务授权。任意命令执行、任意路径文件 API 不能伪装成残留“原生能力”。

## 6. 验收

### 静态门禁

1. 业务服务/core/kernel 依赖树零 Tauri；不依赖桌面 AppState/AppHandle。
2. 桌面业务调用全部经共同 RPC client；Tauri command/插件能力为可审计 OS 白名单，无领域 dispatcher、PTY、业务持久化、Agent/Host 后台 owner。
3. CLI LAN/Cloud/TUI 与桌面使用共同 handler/事件定义；能力清单无未分类命令、无桌面专属业务 fallback、无功能以“暂不支持”静默消失。

### 真进程与多客户端

1. 桌面、CLI、Remote 同时连接同一 Runtime，观察同一 workspace/pane/Agent 身份及事件顺序；各自 selection/activation 隔离。
2. 启动真实 PTY 与 Agent，强制退出桌面进程，再用 CLI/Remote 继续输入、读取输出、查看审批与恢复；重开桌面绑定原实体，不能重新创建冒充恢复。
3. 关闭/最小化/后台桌面不影响 Host 或其他客户端。独立服务重启则显式处理新代际；不声称已死 PTY 自动续命。
4. 文件/Git/搜索/watch、主题/设置、Hosts/Cloud、Agent/Communion/Hub、审批与恢复逐项回归；多窗口与重连、取消、超时、服务不可达、版本冲突、重复请求、跨 workspace、越权均有断言。
5. 旧用户数据真实迁移并重新读取；中断/权限失败/冲突时无丢失、无双写，备份和归档可识别并可恢复。

### 流与性能

同一构建与可比负载实测桌面、CLI、Remote 的真实 child reply；记录输入/控制队列、parse/feed/render、P50/P95/P99/max、成功数、超时数与诊断开销。保留 FIFO、delta 顺序、history/replay 有界与 active-only 订阅，桌面不得新增镜像转发或重复解析流。覆盖空闲、持续输出、切换、历史、重连、Host 断开与后台；实际 WAN/蜂窝未测须明示，不把本地通过提升为公网性能通过。不以绕过 Tauri 自动推断延迟已下降，不自行杜撰性能阈值。

实施交付运行 npm test、npm run lint、npm run typecheck，相关 Rust unit/integration、真实桌面/CLI/Remote 集成与架构门禁；保存实际证据，再执行 SpecTree verify 与 complete。此设计轮未修改业务代码、未运行上述实施测试。

## 7. 应用与范围协调

新契约节点 `L2-UNIFIED-CLIENT-RUNTIME` 位于项目根下；配套 JSON 的正文由本文件生成，避免两份人工规范。引用现有 `L2-REMOTE-001`、`L2-TERM-001`、`L2-PROTO-001`、ACP 与 Communion 契约；正式应用前逐项更新其中仍声明 shell/Tauri 为权威的正文及与冻结 Runtime 的冲突，保留历史来源，不用新节点遮盖冲突。

实施路径分组见 JSON。新文件/新 crate 或新增协议 surface 必须先通过 SpecTree 纳入实际 allowedPaths；候选路径不是当前代码写入授权。最终锁应基于 CHG-069 完成后的真实调用链重新收窄；不能借 broad glob 顺手重构无关模块。
