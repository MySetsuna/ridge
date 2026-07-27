# PROJECT_STATUS.md

> 由 NotebookLM 来源导出归档（2026-07-23），原 char_count=13814

Ridge 项目现状：产品、架构、仓库与规划基线

状态日期：2026-07-21




覆盖仓库：

wind

 与兄弟仓库 

ridge-cloud




用途：作为人类与 NotebookLM 的单一“当前现状”来源，辅助规划、取舍与追问。




不含：密钥、生产凭据、用户数据；也不把历史计划或未复测功能写成已验证事实。

0. 如何读取这份文档

本文采用“逐层展开”，避免把 NotebookLM 淹没在实现细节中：

先读第 1、2、9 节，理解产品、边界与当前风险。

需要方案设计时，再读第 3～8 节的数据流、契约与能力状态。

只有定位实现时，才使用第 11 节的代码证据索引。

历史文档只在第 12 节保留压缩结论；任务流水、评审过程和过期进度不再作为当前事实。

证据等级：

代码事实

：由 2026-07-21 全量重建后的 CodeGraph 和当前源码确认。

Git 事实

：由本地分支、HEAD、工作区和提交历史确认。

文档声明

：来自协议或运维文档；若与代码冲突，以代码为当前行为、以协议为应修正目标。

运行事实

：必须有本轮测试、生产探测或发布证据。缺证据时明确写“未验证”。

1. 一页结论

Ridge 当前不是单一终端应用，而是一套“本地优先、可远程接入、支持智能体协作”的终端工作空间：

wind

 提供桌面应用、终端内核、工作区/Pane、文件与 Git、LAN/公网远控、

rdg

 CLI、tmux 兼容层及 teammate/MCP 协作。

ridge-cloud

 提供账户、设备、订阅、租户域名、WebRTC 信令、TURN/ICE、管理端，以及 Remote Web 产物的独立上传与激活。

本地、LAN 与公网三种入口正在收敛到共享的 

ridge-core

、

ridge-remote

、

@ridge/remote

 与 

ridge-term

，但迁移仍处于“主体已共享、边缘仍有宿主适配”的阶段。

公网远控的业务数据走 WebRTC DataChannel 和端侧 E2EE；

ridge-cloud

 负责认证、授权、房间与 SDP/ICE 转发，不应看到终端明文。

历史文档量远超当前信息需要，且至少存在一处关键漂移：

wind/docs/contracts/ridge-cloud-protocol.md

 明显落后于 

ridge-cloud/docs/ridge-cloud-protocol.md

。后者应作为唯一协议 SSOT。

当前最大不确定性不是“有没有代码”，而是“本轮没有可运行的完整绿灯与生产验证”：沙箱中缺 

pnpm

，Rust stable toolchain manifest 也不完整；本轮未验证线上 Dokku、TURN、持久卷激活版本。

2. 仓库与交付快照

2.1 

wind

项

当前值

路径

C:\code\wind

分支 / HEAD

develop

 / 

e981cf5b6fd234c5c544be9afaf38431d0188179

相对远端

比 

origin/develop

 领先 2 个提交

应用版本

0.0.16

（

package.json

）

CodeGraph

532 文件、11,689 节点、40,494 边；当前引擎全量重建

主要语言

TypeScript/Svelte + Rust；终端内核编译为 WASM

工作区说明

原有 

.codegraph/.gitignore

 修改与若干本地目录/文件；本轮不覆盖用户未提交内容

最近两笔领先提交用于固化 Ridge 发布工作流，不等于已经完成本轮发布。

ridge-code/

 是父仓库未跟踪的外部工作副本，已从父仓库 Git/CodeGraph 噪声中排除；本文件不把它当作 

wind

 已提交组成部分。

2.2 

ridge-cloud

项

当前值

路径

C:\code\ridge-cloud

分支 / HEAD

develop

 / 

1d7f5b8bfe6804f459c30e28fdc8fdfb2628704e

相对远端

与 

origin/develop

 一致

CodeGraph

117 文件、1,848 节点、4,646 边；当前引擎全量重建

数据层

PostgreSQL + SQLx；14 个顺序迁移文件

前端

主 Web、租户 controller、管理端三类 SPA；租户 controller 再按 UA/覆盖参数分桌面与移动产物

工作区说明

.codegraph/

 与 

docs/handoff-admin-session-live-duration.md

 未跟踪；本轮保留

仓库配置了 

origin

 与 Dokku remote，但“有 Dokku remote”不代表当前生产 SHA、健康状态、TURN 端口和持久卷产物已在本轮验证。

3. 产品边界与核心原则

3.1 产品边界

Ridge 的当前产品面可分五层：

终端工作空间

：多工作区、多 Pane、PTY、终端渲染、搜索、历史、主题、文件与 Git。

宿主能力

：Tauri 命令、进程/文件系统/剪贴板/LSP、持久化状态、平台打包。

共享内核

：

ridge-core

 统一命令与宿主端口；

ridge-term

 统一终端语义；

ridge-remote

 / 

@ridge/remote

 统一远控协议与 UI 适配。

多入口

：桌面 GUI、

rdg

 CLI、本地/LAN Web、官方公网 Web。

协作与云服务

：teammate/tmux/MCP，以及 

ridge-cloud

 的身份、授权、信令、TURN、计费和静态入口。

3.2 现有代码体现的原则

本地操作优先；云端不成为终端明文和 PTY 状态的中心数据库。

相同工作区/Pane 能力尽量经共享内核暴露，入口只做适配。

远控数据面与控制面分离：云端认证/信令，端侧 WebRTC/E2EE 传业务帧。

设备“接入”与远控“激活”分离：host 可上线；controller 权限按数据库实时订阅/体验状态判定。

协议、安全与资源上限是硬边界；动画、装饰和新增抽象不能凌驾于正确性与可维护性。

4. 

wind

 当前架构

4.1 桌面与终端主链路

主数据流：

Svelte 页面/组件
  → Tauri invoke 或事件订阅
  → src-tauri commands / ridge-core dispatch
  → 工作区状态 + PTY engine
  → pane 原始输出或 GridDelta
  → @ridge/remote TerminalManager
  → ridge-term WASM TerminalKernel + Renderer
  → Canvas 终端画面


text

关键职责：

src/routes/+page.svelte

 组装工作区、侧栏、远控与 Agent Center。

SplitContainer.svelte

 / 

RidgePane.svelte

 负责 Pane 布局和单终端交互。

packages/remote/src/shared/terminal/manager.ts::TerminalManager

 统一终端实例生命周期、输入、resize、selection、search、IME 和绘制调度；桌面与 Remote 复用。

packages/ridge-term

 实现 parser、grid、scrollback、selection、search、增量绘制和 WASM 绑定。代码中存在 Canvas2D 与 WebGPU 路径；默认/生产体验仍应按 Canvas2D 已知路径理解，WebGPU 需要新的真机证据。

src-tauri/src/engine

 管理 PTY、Pane tree 与解析；

src-tauri/src/commands

 暴露宿主能力。

packages/ridge-core

 已接管大量 workspace、pane、Git 只读/写入与异步 dispatch，但 Tauri 仍保留必要的宿主状态、平台资源和事件桥。

4.2 工作区、编辑与 Git

已存在的用户能力包括：工作区/Pane 创建与切换、文件树、Monaco 编辑、Markdown 预览、图片预览、Git 状态/历史/差异、LSP 诊断、主题和工作区壁纸。它们不是独立产品，而是围绕“终端工作空间”组织。

当前架构风险在于部分能力同时存在于前端 store、Tauri command 与 

ridge-core

 端口中。后续重构应先确认所有入口和调用者，再减少双路径；不要为追求“纯内核”而把平台特有能力强塞入通用层。

4.3 

rdg

 CLI 与无头模式

packages/ridge-cli/src/main.rs

 当前暴露：

tui

：本地交互式终端界面；

login

：账号登录并绑定设备；

remote

：以已激活设备凭据运行公网 host daemon；

connect

：作为 controller 连接 LAN host；

tmux

：托管无头 

ridge-tmux

 会话。

CLI 与桌面共享部分 

ridge-core

 / 

ridge-remote

 能力，但 UI、平台生命周期和凭据存储仍各有适配层。规划时应优先消除协议和行为差异，而非强行合并界面代码。

4.4 Teammate、tmux shim 与 MCP

当前链路大致为：

外部智能体调用 tmux/MCP
  → 打包的 tmux shim 或 Ridge MCP server
  → teammate server / layout_event / ridge-tmux
  → 工作区与 Pane 变更
  → Svelte AgentCenterPanel / 分组 / HITL 审批反馈


text

已实现面包括 tmux 常用命令映射、端点重发现、拓扑展示、leader/worker 状态、手动分组、Agent 自助入组、危险操作 HITL 和熔断信息。未知 tmux 子命令目前存在兼容性成功回退，故“命令返回成功”不总等价于完整 tmux 语义已实现；规划和测试应以具体行为断言为准。

5. 远控架构

5.1 三种入口，一个目标模型

入口

控制面

数据面

当前定位

本地桌面

Tauri 进程内命令/事件

本机 PTY

完整宿主与控制界面

LAN Web

host 内置 HTTPS/WSS server

局域网 WS，共享 Remote 协议

无云依赖的同网远控

公网 Web

ridge-cloud

 认证与信令

WebRTC DataChannel + E2EE

跨网、移动与多控制方

三者应共享 Pane/工作区 RPC、终端内核、历史语义与错误分类；差异只应来自传输、身份和宿主权限。

5.2 LAN 路径

桌面/CLI host 通过共享 

ridge-remote

 server 提供 HTTPS/WSS、TOTP 或 session 鉴权、Pane 订阅、输入、resize 和工作区 RPC。

RemotePanel.svelte

 展示二维码、TOTP、LAN 会话、断开和黑名单。浏览器端通过 Tauri shim 形状的 transport 复用桌面组件或轻量移动组件。

5.3 公网路径

主链路：

host: RidgeCloudHost(device JWT)
  ↔ ridge-cloud /ws（认证、授权、房间、SDP/ICE 转发）
controller: ControllerCloudProvider(user access JWT)
  ↔ WebRTC DataChannel
  ↔ E2EE session
  ↔ CloudHostBridge
  ↔ 本机 invoke / Pane 输出源


text

代码确认的关键特性：

host 是 answerer；controller 是 offerer。

一个房间为 1 host + N controller，每个 controller 有随机 

cid

 定向寻址。

controller 的同 

cli

 新连接可顶替旧连接，服务网络切换/刷新重连。

controller 和 host 均支持退避重连、ICE 重建、连接看门狗；Pane 历史采用首屏小预算和滚顶懒加载。

DataChannel 业务帧端侧加密，并有分片/重组与发送缓冲背压上限。

CloudHostBridge

 在验证完成前门控 invoke 与 Pane 订阅，支持 TOTP、信道绑定 TOTP、trusted-controller 和 E2EE 临时公钥绑定钩子。

须注意：若某些 verifier 未注入，桥为兼容旧路径可能默认放行或降级。故“代码支持安全钩子”不自动证明每个生产入口都已启用；需要以构造点、配置与 E2E 证据确认。

6. 

ridge-cloud

 当前架构

6.1 进程与路由

ridge-cloud

 是单个 Rust/Axum 服务，承担 API、WebSocket relay 和多套静态 SPA 托管。

src/router.rs::build_router

 汇总路由与中间件；

spa_fallback

 按 Host、租户解析、UA 与 

?ui=

 选择：

主域：账户/设备 Web；

admin.{base}

：管理端；

{device}-{username}.{base}

：租户 controller；桌面与移动产物分流；

PWA 根资产有专门规则，避免 

sw.js

 被 SPA fallback 错返为 HTML。

tenant_resolver

 同时处理 HTTP/1.1 Host 与 HTTP/2 authority，非法租户形状返回 404，合法租户注入 request extension。

6.2 身份、设备与计费

JWT 有 

user

 与 

device

 scope；新签发优先 EdDSA(Ed25519)，可在迁移期继续验 HS256。

浏览器用父域 

ridge_sso

 HttpOnly cookie 维持 refresh 会话，再换短时 access token；数据库只存 refresh token hash。

设备有 enabled/parked 状态和按用户组配置的配额；WebSocket 建连时按数据库实时状态判定，不信任长寿命 JWT 中可能过期的 plan claim。

controller 需要远控权限，host 上线不等于 controller 已获授权。

付费链路包括 Lemon Squeezy 与爱发电；签到体验与真付费会员必须分开判断。

管理端提供用户、会话、组配额等控制面。

6.3 WebSocket 信令与 TURN

ws_upgrade

 在升级前后执行租户、token、role/scope、用户名、设备归属、parked 状态、订阅权限、全局/用户/组连接上限等校验。匿名无权请求尽量返回不透明错误；已认证但无权的客户端可收到结构化 WS error 帧。

房间 key 使用已验签 

user_id

 与设备名，而不是可猜的明文用户名命名空间。relay 只路由信令消息；业务明文不应进入服务端。

GET /api/v1/ice-servers

 总是可返回 STUN；同时配置 

TURN_HOST

 与 

TURN_STATIC_AUTH_SECRET

 才追加 coturn 时效凭据。生产移动网络是否稳定，仍取决于 TURN 3478 TCP/UDP 与 relay 端口段真实可达。

6.4 数据与安全中间件

PostgreSQL 经 SQLx runtime query 访问；启动执行嵌入式顺序 migration。

启动数据库连接有有限重试；必需配置缺失或 JWT 密钥配置不完整时 fail-fast。

API 使用统一 

{ok,data}

 / 

{ok:false,error}

 信封；支付 webhook 按供应商约定例外处理。

CORS、请求体上限、通用/认证限流、WS 并发上限、安全响应头与敏感错误脱敏构成外层防线。

6.5 Remote Web 产物与应用部署解耦

自 2026-07-11 起，

desktop-app

 / 

mobile-app

 不再依赖每次重建 

ridge-cloud

 Docker 镜像：

wind

 构建两套 Remote 产物并生成带 manifest 的长度前缀 bundle。

发布脚本用专用 

RIDGE_ARTIFACT_TOKEN

 上传到 

/api/v1/remote-artifacts

。

ridge-cloud

 校验体积、路径安全与两套 

index.html

，写入持久卷 

releases/<version>

。

激活通过 current 指针切换 desktop/mobile，保留最近 3 个 release 供回滚。

上传路由刻意置于普通 API 20 秒 timeout 之外，但仍受独立体积上限和 token 守卫。

这意味着生产状态有两条独立版本线：

ridge-cloud

 代码 SHA 与激活的 Remote artifact version。只验证其中一条不能判定完整发布成功。

7. 当前能力成熟度

能力

状态

证据与边界

桌面工作区 / Pane / PTY

已实现

主路径广泛使用；本轮未跑回归

ridge-term

 parser/grid/scrollback/render

已实现

WASM/host 代码和大量历史测试存在；本轮工具链阻塞

Canvas2D 渲染

主路径

当前可靠基线；仍需真机性能/视觉门禁

WebGPU 渲染

实验/未复验

代码实现存在；历史计划明确要求浏览器实跑

文件、Monaco、Git、LSP

已实现

多组件与 command/core 路径；完整 IDE 回归未跑

LAN Remote

已实现

共享 server 与 UI 存在；证书/真机网络未复验

Cloud Remote 多控制方

已实现

provider、relay、cid、重连、分片、懒历史均有代码

Cloud 端到端安全启用度

部分待核

能力齐全，但 verifier/生产配置须逐构造点和 E2E 确认

rdg

 本地/LAN/公网/tmux

已实现但入口多

命令面存在；需行为一致性矩阵

Teammate/MCP/HITL

可用且继续演进

拓扑、分组、tmux shim、审批已接；tmux 全语义不是目标事实

多主机 foreign terminals

基础层/未完全验证

注册与抽象存在；不要假定所有 live transport 已闭环

Cloud 账户/设备/计费/管理

已实现

后端、三 SPA、迁移存在；支付与邮件需生产配置验证

Remote artifact 独立发布

已实现

代码与提交完整；本轮未核生产持久卷当前指针

Ridge 完整发布流程

有固化流程

不等于已执行；发布时须走专用 release skill 与证据门禁

8. 不可破坏的契约与安全不变量

8.1 协议 SSOT

唯一权威协议应为：

C:\code\ridge-cloud\docs\ridge-cloud-protocol.md

。

本轮比较发现 

wind/docs/contracts/ridge-cloud-protocol.md

 比云仓版本少大量 SSO、EdDSA、设备配额、错误帧和多控制方内容，差异为 260 行新增、162 行删除。继续维护两个手写“SSOT”会误导代码生成与评审；

wind

 侧只应保留指向权威协议的短入口，不再保留独立全文。

8.2 必须持续成立的技术不变量

relay 不读取终端业务明文；业务帧只在端侧加解密。

host 与 controller 必须同账户，role 必须匹配 JWT scope 和设备。

房间、配额与付费权限使用已验证身份和数据库实时状态。

TOTP/受信设备状态必须在业务帧门控前完成，失败次数有上限。

远控 resize、输入、Pane 订阅与 scrollback 语义应跨 LAN/cloud/CLI 一致。

文件路径、artifact 路径、上传体积、WebSocket 帧与连接数必须有限制。

migration 只追加，不改写已发布历史。

日志不得输出 token、TOTP seed、私钥、完整敏感查询或 

RIDGE_ARTIFACT_TOKEN

。

发布不能把 

ridge-cloud

 SHA 与 Remote artifact version 混为一个版本。

9. 当前最高优先级风险与决策点

P0：先建立可信基线

恢复可运行门禁

：当前沙箱无 

pnpm

，Rust stable toolchain manifest 缺失。本轮无法给出新的绿灯。下一次开发前至少恢复 

pnpm check

、

pnpm test

、

cargo test --workspace

 与 

ridge-cloud cargo test

。

消除协议双 SSOT

：云仓协议保留权威全文；

wind

 只留入口。协议变更必须先改权威契约，再改服务端与所有客户端，并跑 drift/conformance 测试。

分开验证两条生产版本线

：记录 Dokku 

main

 SHA、HTTP health、Remote activated version、desktop/mobile 持久卷文件；缺一项不宣称发布完成。

P1：收紧边界，减少双路径

确认云安全钩子的生产构造点

：逐项证明 key binding、TOTP bind、trusted controller、帧门控在 desktop host、CLI host、desktop controller、mobile controller 上的实际启用状态。

完成共享内核迁移的减法审计

：按调用图找仍重复实现的 workspace/pane/Git/remote handler；只删除有等价共享路径和回归覆盖的重复面。

建立跨入口行为矩阵

：同一 RPC 在 desktop、LAN Web、cloud desktop、cloud mobile、

rdg

 上应有明确支持/拒绝/降级，而不是靠入口偶然差异。

把 artifact 运维状态显式化

：至少能查询 current version、保留 release 和 manifest SHA；规划发布/回滚时先读状态，勿盲目覆盖。

P2：以真实价值决定是否继续

WebGPU

：先做单 Pane 真机正确性与可量化性能对比；无显著收益则保持实验或删除，不继续堆共享 surface 架构。

多主机与 teammate

：优先验证真实用户工作流、故障恢复和可理解性；勿因已有抽象而默认扩张协议面。

IDE 能力

：文件/Git/LSP 继续围绕终端工作流补闭环；避免追求 VS Code 功能清单式对标。

10. 建议 NotebookLM 如何辅助规划

NotebookLM 应把本文件视为“当前基线”，而不是产品需求全文。每次规划回答应：

先引用相关现状、风险和证据等级。

明确区分：已实现、历史声称、未验证、拟议。

同时给出“新增、简化、合并、删除”选项；减法与加法同权。

优先解决 P0/P1 的正确性、可验证性和双路径，而非新增展示性功能。

为每个建议给出可判定验收信号：测试、退出码、协议一致性、生产状态或真机指标。

若结论依赖某个符号或线上状态，本文件证据不足时必须要求 CodeGraph/测试/生产探测，不得脑补。

不把历史日期、旧完成度、旧测试数或计划中的“✅”当作当前运行证据。

推荐提问模板：

基于《Ridge 项目现状》，先指出与目标直接相关的当前代码路径、已知风险和缺失证据；再给 1～3 个方案，包括至少一个减法方案。按价值、复杂度、风险、可验证性排序，并给出明确不做范围与验收信号。若来源不足以支持结论，列出必须补查的 CodeGraph 符号或运行状态。

11. 代码证据索引

wind

主题

关键路径 / 符号

桌面组合

src/routes/+page.svelte

, 

src/lib/components/SplitContainer.svelte

, 

RidgePane.svelte

终端协调

packages/remote/src/shared/terminal/manager.ts::TerminalManager

终端内核

packages/ridge-term/src/term/terminal.rs::Terminal

, 

packages/ridge-term/src/lib.rs::JsTerminal

PTY/Pane

src-tauri/src/engine/pty.rs

, 

pane_tree.rs

, 

src-tauri/src/commands/pane.rs

共享内核

packages/ridge-core

, 

dispatch

, workspace/pane/Git ports

LAN Remote

packages/ridge-remote

, 

src-tauri/src/remote_bridge.rs

, 

RemotePanel.svelte

Cloud controller

packages/remote/src/shared/cloud/controllerCloudProvider.ts::ControllerCloudProvider

Cloud host

ridgeCloudProvider.ts::RidgeCloudHost

, 

cloudHostStore.ts

Cloud RPC/Pane bridge

cloudHostBridge.ts::CloudHostBridge

CLI

packages/ridge-cli/src/main.rs::Command

tmux/teammate

packages/ridge-tmux/src/lib.rs

, 

src-tauri/src/bin/tmux.rs::dispatch

, 

src-tauri/src/teammate

, 

AgentCenterPanel.svelte

Artifact 发布

scripts/publish-remote-cloud.mjs

, 

package.json::publish:remote-cloud

ridge-cloud

主题

关键路径 / 符号

启动与路由

src/main.rs

, 

src/router.rs::build_router

, 

spa_fallback

租户与安全头

src/middleware.rs::tenant_resolver

, 

security_headers

JWT/身份

src/auth/jwt.rs::JwtCodec

, 

src/auth/extract.rs

SSO session

src/db/auth_session_repo.rs

 与 auth routes

设备与配额

src/db/device_repo.rs

, 

device_quota

, device routes

信令升级

src/ws/handler.rs::ws_upgrade

房间/多控制方

src/ws/rooms.rs::RoomRegistry

TURN/ICE

src/api/ice.rs::ice_servers

Remote artifacts

src/api/remote_artifacts.rs::{upload,rollback,write_release,activate}

配置

src/config.rs::Config::from_env

数据库

src/db/pool.rs

, 

migrations/*.sql

12. 历史文档归档摘要与保留策略

清理前，

wind

 有 138 个已跟踪 Markdown，其中 

docs/specs/**

 56 个、

docs/plans/**

 50 个、

docs/term-rebuild/**

 11 个、

docs/_team-archive/**

 3 个；

ridge-cloud

 有 13 个 Markdown，其中历史 spec/plan 5 个、handoff 2 个、review notes 1 个。主要长期结论已压缩如下：

Terminal 重建

：自研 Rust/WASM kernel 是当前事实；scrollback、selection、resize、TUI、IME、增量渲染是核心不变量。逐轮 bug 清单和旧测试数字不再保留为现状。

统一 Remote

：LAN/cloud/CLI 共享协议与终端层；公网采用信令与数据面分离、多 controller、懒加载历史、分片和重连。各阶段实施计划不再保留。

Cloud 安全与身份

：父域 SSO、短 access token、EdDSA 迁移、TOTP/绑定、设备 parked/配额、TURN 是长期决策。旧审计“已修/待修”标签不作为当前证据。

Teammate

：tmux shim + 本地 MCP + topology/group/HITL 是当前方向；四 Domain 愿景稿中的未实现设想不自动进入 backlog。

ridge-core

 内核化

：保留“共享业务语义、宿主提供端口”的边界；不以迁移比例为目标，避免把平台细节下沉。

Remote artifact 解耦

：应用代码部署与 desktop/mobile Remote 产物激活是两条发布线，必须分别验证。

UI/IDE 设计

：已落地行为由代码和 README 说明；逐功能设计稿、QA 交接和实现流水不再作为长期文档。

清理后的常驻文档应只包括：

本文件 

docs/PROJECT_STATUS.md

；

根 

README.md

 / 

docs/README_CN.md

 / 

CHANGELOG.md

；

用户与运维指南：Remote/CLI/MCP/teammate/CDP/release signing 等仍在使用的说明；

包级 README、测试环境 README、许可证说明；

ridge-cloud/docs/ridge-cloud-protocol.md

 唯一协议全文；

精炼后的 

AGENTS.md

，以及只作入口的 

CLAUDE.md

。

历史全文不复制到新“archive”目录，避免把噪声换个位置继续喂给模型；需要审计时从 Git 历史恢复：

wind

 清理前基线：

e981cf5b6fd234c5c544be9afaf38431d0188179

ridge-cloud

 清理前基线：

1d7f5b8bfe6804f459c30e28fdc8fdfb2628704e

13. 刷新规则

发生以下任一事件时更新本文件，并在 NotebookLM 中替换旧来源，不叠加多个版本：

跨仓协议、身份、安全边界或 Remote 数据流改变；

ridge-core

 / 

ridge-remote

 / 

ridge-term

 的所有权边界改变；

发布架构、生产域名、持久卷或回滚机制改变；

一项“部分/未验证”能力获得或失去确定性证据；

P0/P1 风险关闭、新增或优先级变化。

普通 bug 修复、局部 UI 调整和逐任务过程不应写入本文件；只有它们改变稳定能力、架构边界或规划判断时才更新。
