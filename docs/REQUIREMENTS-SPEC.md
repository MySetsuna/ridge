# Ridge · 需求规范（REQUIREMENTS-SPEC）

> 本地只保留此一份需求文档。Pending 未获用户明确批准前，不改代码、不生成执行合同、不上传；
> NotebookLM 继续使用上一版已批准的 `REQUIREMENTS-SPEC` 来源。

## 正式需求 (Active Requirements)

### REQ-REMOTE-HOST-TREE-01 · 公网同账号 / LAN 直连主机与三层拓扑

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 类型：`FIX`
- 原始意图：接入公网或局域网远端主机后，读取现有 pane，以“主机 → 工作区 → pane”树展示并管理。
- 目标行为：
  - 公网 cloud “接入主机”仅允许同一 Ridge 账户；LAN 不限制账户归属，沿用 LAN 自身的 TOTP/session/E2EE 认证。
  - 主机菜单：刷新、添加工作区、断开/忘记；工作区菜单：打开、添加 pane、重命名、保存、分享、关闭；pane 菜单：接入/聚焦、标记 Agent、切 shell、删除。
  - “删除 pane”仅删除目标远端 pane/PTY，须二次确认；不得连带删除 workspace 或其他 pane。
  - 删除成功后，以该控制端当前接入的同源 host pane 为引用计数：若为 `0`，断开该控制端与 host 的连接；若仍有至少一个 pane，保持连接。删除失败不减计数、不触发断开。
  - 菜单逐项受 host capability 与调用者 scope 门控；不支持项不展示。
- 范围：桌面 Hosts 侧栏、LAN/cloud controller 连接、host topology DTO、远程 workspace/pane CRUD 与确定性测试。
- 非目标：跨主机拖拽 pane、远端 pane 像素预览、绕过 LAN TOTP/E2EE、让不同账户经公网 cloud 取得整机视图。
- 不可动边界：relay 不见 PTY 明文；公网同账号整机门禁由 cloud 与 host 双重校验；LAN 不以 cloud 账户关系作门禁。
- 确定性验收：
  - 两台 host fixture 各含多 workspace/pane，树投影隔离且层级正确。
  - 公网同账号 token 可列全树；异账号普通 user token 在 relay 与 host 两端皆拒绝。
  - LAN 不要求账户匹配；有效 TOTP/session fixture 可接入，无效 LAN 凭据仍拒绝。
  - 菜单动作路由到选定 host/workspace/pane，跨 host 污染为零；删后尚有第二个同源接入 pane 时连接保留，删掉该控制端最后一个同源接入 pane 时仅断开一次。
- 预期落点：`packages/remote/**`、`src-tauri/src/hosts/**`、`src/lib/stores/hosts.ts`、`src/lib/components/hosts/**`、`ridge-cloud/src/ws/**`。

### REQ-WORKSPACE-SHARE-01 · 跨账号单工作区分享

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 类型：`NEW`
- 原始意图：用户可只分享一个工作区；它区别于接入整台主机，可授权给不同 Ridge 用户，并统一显示在主机侧栏。
- 目标行为：
  - owner 从工作区菜单创建/撤销邀请；受邀用户登录后，只见该工作区及其 pane，不得枚举同 host 其他工作区、设备或会话。
  - 共享工作区打开时，其每个 pane 的 `cwd` 与本机 `cwd` 同层、同样式出现在资源管理器；可展开目录并打开文件。数据从 origin host 的同一 scoped provider 读取，不把远端绝对路径当成本机路径，亦不投影成本机资源。
  - 共享工作区须同时显示该区对应的 Git 管理与 Agent tab；Terminal、Explorer、Git、Agent 皆绑定同一 `grant_id + workspace_id`。
  - 角色两级：`viewer` 可读 pane 输出、文件树/文件、Git 状态/差异/历史、Agent roster/状态/最近回复；`operator` 另可 stdin、resize、pane CRUD/切 shell、文件写入、既有 Git 写操作与既有 Agent 操作。危险写仍沿用 HITL。
  - Explorer 写权限与主机 tab 同口径：`viewer` 可打开文件但不可保存、新建、重命名、删除或粘贴；`operator` 可用现有 Remote 文件能力。UI 收敛写入口，origin host 仍须逐请求拒绝越权。
  - 若首版无法可靠贯通 `viewer` 只读门禁，桌面端可仅提供 `operator`，其体验等同正常 Remote、但范围仍限获授 workspace；不得提供名为 `viewer`、实可写入的伪只读角色。
  - 二者均不得添加/关闭工作区、转分享、改 host 设置，亦不得在控制端借 Remote/Hosts 功能二次转发、导出或分享该资源。
  - Hosts 侧栏统一投影：整机接入显示普通 host 全树；分享显示“共享：工作区名 · owner/host”受限根节点，只含一个 workspace。
  - Ridge 桌面 owner 入口共用一个 `WorkspaceShareDialog`：本机工作区在 Explorer 工作区标题 `⋯ → 分享工作区` 与 WorkspaceTree 工作区行 `⋯ → 分享工作区`；已接入主机则在 Hosts 侧栏 owner 工作区 `⋯ → 分享工作区`。共享投影自身不显示分享入口。
  - 分享邀请替代 TOTP 知识传递；controller 取得短期 `workspace_share` capability token，relay 与 host 依据 `grant_id + workspace_id + role` 双重门控。
  - ridge-cloud 负责 grant/invite 持久化、账号解析、owner/device/workspace 绑定校验、短期 scoped token、WS room 路由、邀请/撤销事件与到期/撤销踢线；不读取文件、Git、Agent 或 PTY 明文。桌面 host 负责 workspace 存在性与逐 RPC/事件授权。
- 范围：ridge-cloud grant/invite 数据模型与 API、scoped JWT、WS room 入场、host 每-controller scope、Remote RPC/事件过滤、Hosts 树与权限菜单、Explorer/Git/Agent scoped adapter、撤销/过期与非转授门禁。
- 非目标：匿名公开链接、无账户访客、转分享、分享整个 host、跨 workspace 文件访问、首版 LAN P2P 优化。
- 不可动边界：
  - grant 绑定 `owner_user_id + device_id + workspace_id + grantee_user_id`；短 token 不作为数据库真相源。
  - host 不信任 relay 单点裁决；每条 RPC、pane 订阅、事件广播均按 controller scope 再校验。
  - 首版 workspace share 走 cloud relay；即使双方同 LAN，亦先完成 cloud 身份/授权，不降级匿名直连。
  - 文件/搜索路径由 origin host canonicalize 后强制位于该 workspace root；symlink、`..`、绝对路径不得越界。Git repo 亦须属于该 root。
  - shared workspace 只存 remote projection，禁止写入控制端 `AppState.workspaces`、host export inventory 或任一 Remote host room；capability token 固定 `delegable=false`。
- 假设/待确认：
  - 优先交付 `viewer/operator`，默认 `viewer`；若只读资源面无法可靠闭环，则首版只开放 `operator`，后续补 `viewer`。
  - 邀请按 Ridge username/email 定向，不提供 bearer link；默认 7 天待接受、已接受 grant 可设到期或永久。
  - owner 关闭 workspace 或撤销 grant 时，服务端立刻踢出对应 controller，host 清订阅。
- 确定性验收：
  - grantee A 仅能列获授 workspace；访问 sibling workspace、创建 workspace、转分享均返回稳定 `SCOPE_DENIED`。
  - viewer 可见该区 Explorer/Git/Agent 读面；写/改操作全拒；operator 仅获该区既有写白名单；owner 原能力不回归。
  - 共享 cwd 与本机 cwd 同层渲染且文件可打开；viewer 保存/新建/重命名/删除/粘贴皆拒，operator 写入仅落 origin workspace root。若采用 operator-only 降级，UI/API 均不得出现 viewer。
  - 路径遍历、workspace 外 symlink、跨 repo Git、跨 workspace Agent 操作皆稳定拒绝。
  - 控制端启用自身 LAN/public Remote 或 Hosts 服务后，shared workspace 不出现在其任何 host topology；使用 share token 充当 host/controller owner 皆拒绝。
  - grant 撤销/过期后既有连接被踢、重连失败；token 重放不恢复权限。
  - UI 同屏展示同账号 full-host 与跨账号 shared-workspace，菜单随 role 精确收敛。
  - 桌面 Explorer、WorkspaceTree、Hosts owner 工作区三入口打开同一分享对话框；同一 grant 的创建、改角色、撤销结果一致。
- 预期落点：`ridge-cloud/migrations/**`、`ridge-cloud/src/{api,auth,db,ws}/**`、`packages/remote/src/shared/cloud/**`、`src/lib/components/hosts/**`、`src/lib/stores/hosts.ts`、host bridge policy/tests。

### REQ-WORKSPACE-SAVED-01 · 已保存工作区可重开、可删除及滚动条统一

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 类型：`FIX`
- 原始意图：已保存工作区打开并使用、关闭后须能再次载入；已保存列表须能删除文件，滚动条须与应用一致。
- 行为：
  - 关闭工作区须同时清理其 pane 的前端 PTY bridge、terminal kernel 与标题等 runtime；再次打开同一 `.ridge` 内复用的 pane UUID 时必须创建新 PTY，不得误复用 parked runtime。
  - “已保存工作区”弹层可删除默认保存目录内的 `.ridge`，无论该文件是否仍关联已打开工作区；删除前确认，成功后原位刷新列表。
  - 弹层滚动区复用应用通用 `rg-scroll` 样式。
- 边界：按路径删除仅允许默认 `~/ridge-workspaces/` 的直接 `.ridge` 文件；拒绝目录外、子目录、其他扩展名及 symlink 越界；不删除工作区 cwd 内项目文件。
- 验收：前端测证明关闭两 pane 工作区逐一 teardown/detach；Rust 测证明仅默认目录直接 `.ridge` 可删；`svelte-check`、Rust check 通过。
- 追踪：`src/lib/stores/paneTree.ts`、`src/routes/+page.svelte`、`src-tauri/src/commands/ridge_file.rs`、`src/app.css`。

### REQ-REMOTE-03 · 桌面浏览器 Remote pane 尺寸与指针命中一致

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 类型：`FIX`
- 原始意图：桌面浏览器经 LAN 或公网 Remote 时，pane 展示尺寸、终端实际 rows/cols 与鼠标点击位置须一致。
- 行为：
  - LAN 与公网共用一套 pane 几何真相：以最终内容区 `getBoundingClientRect()`、实际 padding、cell 尺寸与 `devicePixelRatio` 计算 viewport、rows/cols、resize/claim。
  - 鼠标/触控坐标须用同一内容区原点与缩放换算映射至终端 cell；不得混用 CSS 像素、设备像素、窗口坐标或旧布局缓存。
  - 首次挂载、分屏拖拽、侧栏变化、浏览器 resize、DPR/缩放变化及重连后均重算；LAN/public transport 不各写几何算法。
- 范围：桌面浏览器 Remote 的 terminal container、共享 renderer/manager、resize/claim 协议与鼠标编码；LAN/public 回归测。
- 非目标：改动原生桌面本机 pane 产品尺寸、重做终端渲染器、以 CSS transform 掩盖后端 rows/cols 错误。
- 不可动边界：几何 SSOT 位于共享 terminal 层；发送给 host 的 rows/cols 与本地 renderer grid 必须同源；修复不得增加无界 ResizeObserver/轮询或 resize 风暴。
- 验收：
  - 纯函数测覆盖 DPR `1/1.25/1.5/2`、padding、分数像素、边界点击，断言 viewport→grid 与 pointer→cell 同源且 clamp 正确。
  - 浏览器 E2E 分别走 LAN/public fixture；初始、分屏拖拽、侧栏开合、窗口缩放后，DOM pane rect、renderer viewport、host rows/cols 一致，鼠标报告 cell 与点击目标一致。
  - resize 合并有界、最终尺寸必达；不得以截图视觉近似代替协议断言。
- 预期落点：`src/remote/**`、`packages/remote/src/shared/terminal/**`、`src/lib/components/RidgePane.svelte`、LAN/cloud Remote resize 与鼠标协议测试。

### REQ-REMOTE-01 · rdg Remote 入口与启停语义

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：rdg 仪表盘 LAN 地址只显示根地址（如 `https://172.21.130.235:9527`），不附 `/login`；LAN Remote 默认停止，须用户显式启动；TUI 内须有名称明确的公网 Remote 启动/停止入口。
- 边界：不改变 `rdg remote` 子命令兼容性；退出 TUI 时仍须回收由本 TUI 启动的服务。
- 验收：dashboard 单测证明根 URL、初始 stopped、无启动 action；菜单文本与动作测试证明 LAN/公网入口明确。
- 追踪：`packages/ridge-cli/src/tui/dashboard.rs` → dashboard tests。

### REQ-REMOTE-02 · rdg LAN 桌面浏览器真正接入

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：桌面浏览器访问 rdg LAN Remote 时直接走 LAN TOTP/session 启动链，完成 WebSocket、workspace、pane、PTY 接入；不得只显示空白桌面壳。
- 边界：公网租户域仍走 Cloud WebRTC/E2EE；LAN 与 Cloud 启动判定不得依赖远端 cloud API 成败。
- 验收：启动判定纯逻辑测试覆盖 LAN IP/localhost 与 cloud 租户/query；LAN host 协议 probe 或等价集成测试证明握手、订阅、stdin 回显。
- 追踪：`src/routes/+layout.svelte`、新增启动判定 helper/tests、`packages/ridge-cli/src/tui/lan_host*`。

### REQ-CLOUD-01 · 公网 Remote 设备配额不得误停 rdg

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：host 与 controller 均按数据库实时用户组计算设备配额；配额自动停用与用户手动停用须分因记录；额度恢复时仅自动启用“因配额停用”的设备。
- 边界：不绕过会员 Remote 权限、设备归属、WS 并发或 controller 数量门禁；手动停用不可被后台 daemon 自动撤销。
- 验收：ridge-cloud 单测覆盖会员 host 不按免费额度降级、quota/manual 两种停用区分、额度恢复只恢复 quota-parked；WS 门控回归全绿。
- 追踪：`ridge-cloud/src/ws/handler.rs`、`src/db/device_quota.rs`、`src/db/device_repo.rs`、顺序 migration。

### REQ-MOBILE-01 · Mobile Remote 弹层、图标与按钮

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：工作区/终端类型切换层通过 portal 挂到 `body`；顶部 Agent 协作入口使用小机器人；Agent 图标后不跟“标记/运行中/启动中”等标记文案；手机 Remote 右侧功能按钮不显示 border 外壳。
- 边界：保留按钮的 `title`/`aria-label`、触控尺寸、焦点与点击行为。
- 验收：Svelte/Vitest 断言 portal action、Bot 图标及无标记文案；移动构建与 svelte-check 全绿。
- 追踪：`src/remote/lib/WorkspaceTree.svelte`、`src/remote/MainApp.svelte`、`src/remote/lib/RemoteSidebar.svelte`、`src/lib/components/SplitContainer.svelte`。

### REQ-AGENT-01 · 全局 Agent Center、pane 状态与最近回复

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：Agent tab 聚合所有已打开工作区的 agents，不以焦点工作区过滤；原顶部操作移入可滚动内容；自动识别的 agent 进程须同步 pane header 与 roster 状态；最近回复从 Claude/Codex JSONL 会话历史提取并显示。
- 边界：工作区目标、编组编辑等写操作仍须显式落到所属工作区；JSONL 扫描须有文件数、单文件读取量与返回条数上限，不上传会话内容。
- 验收：聚合模型与进程识别单测；Rust JSONL fixture 测试覆盖 Claude/Codex assistant 文本、项目过滤与有界排序；pane header UI 不显示尾随标记文案。
- 追踪：`src/lib/teammate/AgentCenterPanel.svelte`、`src/lib/components/RidgePane.svelte`、`src-tauri/src/commands/project.rs`。

### REQ-AGENT-02 · Agent 启动的无头 Shell 发现与唤起

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：经 Ridge tmux shim 由 pane/agent 启动的 native 无头 session 记录创建工作区与 pane；Agent Center 自动列入对应 agent，支持一键召唤到当前工作区；未能归因的普通无头 session 仍可单列。
- 边界：仅承诺 Ridge 持有 PTY master 的 native session 可召唤；任意已脱离 PTY、仅剩 OS PID 的后台进程不可伪装成可接管会话。
- 验收：ridge-tmux 测试证明 creator metadata 从 HTTP header 入 session/list DTO；tmux shim 测试证明工作区/pane header 传播；前端测试证明按 agent 归组与 attach 调用。
- 追踪：`src-tauri/src/bin/tmux.rs`、`src-tauri/src/commands/terminal.rs`、`packages/ridge-tmux/src/{http.rs,lib.rs}`、`src/lib/stores/hosts.ts`、`AgentCenterPanel.svelte`。

## 修订账本 (Revision Ledger)

| 版本 | 日期 | Pending ID | 变更 | 关联/取代 | 批准证据 |
| --- | --- | --- | --- | --- | --- |
| v0.2.0 | 2026-07-27 | `<INITIAL>` | 建立 Remote/Agent 六项需求基线 | - | 用户本线程六项明确落地指令 |
| v0.2.0 | 2026-07-27 | `PENDING-REQ-REMOTE-HOST-TREE-01` | 公网/LAN 主机三层树转 Active | 新增 `REQ-REMOTE-HOST-TREE-01` | 用户明确“将之前 pending 的两个需求通过审批” |
| v0.2.0 | 2026-07-27 | `PENDING-REQ-WORKSPACE-SHARE-01` | 跨账号单工作区分享转 Active | 新增 `REQ-WORKSPACE-SHARE-01` | 同上 |
| v0.2.0 | 2026-07-27 | `<DIRECT-FIX>` | 已保存工作区重开、删除与滚动条纳入需求 | 新增 `REQ-WORKSPACE-SAVED-01` | 用户纠正“要进需求” |
| v0.2.0 | 2026-07-27 | `<DIRECT-FIX>` | LAN/public 浏览器 Remote 几何与命中一致 | 新增 `REQ-REMOTE-03` | 用户明确要求定位修复并并入下一迭代 |
