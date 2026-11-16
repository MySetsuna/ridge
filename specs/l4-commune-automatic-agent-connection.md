---
id: L4-COMMUNE-AUTOMATIC-AGENT-CONNECTION
level: L4
title: 终端 Agent 自动识别与通用协议接入
status: LOCKED
lifecycle: ACTIVE
parent: L3-COMMUNE-PROTOCOL-MEMBERSHIP
code_targets:
  - packages/ridge-kernel/src/agent_profiles.rs
  - packages/ridge-kernel/src/agent_discovery.rs
  - packages/ridge-kernel/src/commune.rs
  - packages/ridge-kernel/src/app_server.rs
  - packages/ridge-acp/src/**
  - packages/ridge-core/src/process_identity.rs
  - src/lib/teammate/TerminalAgentRow.svelte
  - src/lib/teammate/CommuneConnectionDialog.svelte
  - src/lib/teammate/communeMembership.ts
  - src/lib/i18n/messages/commune.ts
  - src/remote/lib/CommuneWorkspaceRoster.svelte
  - packages/remote/src/shared/transport/wsRemote.ts
  - packages/ridge-kernel/src/managed_agent.rs
  - packages/ridge-kernel/src/lib.rs
  - packages/ridge-kernel/src/commune_connection.rs
  - packages/ridge-cli/src/main.rs
  - packages/ridge-cli/src/managed_agent.rs
  - src/lib/teammate/AgentCenterPanel.svelte
  - packages/ridge-core/src/commands/shell.rs
  - packages/ridge-kernel/src/commune_history.rs
  - packages/ridge-kernel/src/managed_console.rs
  - packages/ridge-kernel/src/main.rs
  - packages/ridge-kernel/src/domain.rs
  - packages/ridge-cli/src/tui/lan_session.rs
  - src/lib/teammate/CommuneStartDialog.svelte
  - src/lib/teammate/communeLaunch.ts
  - packages/ridge-kernel/src/server.rs
  - packages/ridge-kernel/src/pty.rs
  - Cargo.lock
  - packages/ridge-kernel/Cargo.toml
  - packages/ridge-kernel/src/commune_endpoint.rs
test_targets:
  - packages/ridge-kernel/tests/agent_catalog_runtime.rs
  - packages/ridge-kernel/tests/commune_registration.rs
  - src/lib/teammate/communeMembership.test.ts
  - scripts/commune-global-ui-acceptance.mjs
  - packages/ridge-kernel/tests/managed_agent.rs
  - scripts/commune-agent-interop-acceptance.mjs
  - src/lib/teammate/communeLaunch.test.ts
acceptance_checks:
  - id: build-standard-stdio-fixture
    command:
      - cargo
      - build
      - --locked
      - -p
      - ridge-acp
      - --bin
      - fake_acp_agent
    description: 先构建唯一受控 stdio 验收进程，供真实管道与生命周期测试使用
  - id: build-collaboration-cli
    command:
      - cargo
      - build
      - --locked
      - -p
      - ridge-cli
    description: 先构建实际终端入口，后续经 CLI、PTY 与 Runtime 验证
  - id: shared-acp-driver
    command:
      - cargo
      - test
      - --locked
      - -p
      - ridge-acp
      - --test
      - existing_conversation
    description: 精确会话、显式鉴权、拒绝鉴权无创建、早期事件、审批与取消
  - id: kernel-protocol-drivers
    command:
      - cargo
      - test
      - --locked
      - -p
      - ridge-kernel
      - --lib
    description: 协议线格式、WebSocket 鉴权、发现误判、历史覆盖及公共 Runtime 回归
  - id: catalog-discovery
    command:
      - cargo
      - test
      - --locked
      - -p
      - ridge-kernel
      - --test
      - agent_catalog_runtime
    description: 已知 Agent、包装命令和通用 ACP 声明的识别，以及未知能力、精确来源与失效引用处理
  - id: existing-session-registration
    command:
      - cargo
      - test
      - --locked
      - -p
      - ridge-kernel
      - --test
      - commune_registration
    description: 精确原会话接入、鉴权与所有权、原组和 agentId 保留，以及失败与重复接入无副作用
  - id: managed-protocol-session
    command:
      - cargo
      - test
      - --locked
      - -p
      - ridge-kernel
      - --test
      - managed_agent
    description: 真实子进程 stdio、唯一会话、桥接接入、权限路由、取消、失联及生命周期无孤立进程
  - id: installed-agent-initialization
    command:
      - node
      - scripts/commune-agent-interop-acceptance.mjs
      - --skip-grok
    description: 真实 Codex、Claude、OpenCode 初始化与正常退出；用户明确豁免本轮 Grok 实机验证，证据须标
      SKIPPED，不能计作通过或完整对话验收。其他缺适配器、鉴权或创建失败仍失败。
depends_on:
  - L4-COMMUNE-ORGANIZATION-MEMBERS
---

# 自动识别与通用协议接入

## 需求与批准状态
用户明确要求自动识别 Codex、Claude、Grok Build、OpenCode，并覆盖可扩展 Agent 与支持 ACP 的 Agent。保留原生独立终端、原进程与原会话，以及 CHG-092 的 agentId 和原组连续性。用户回复“全部授权通过”，已批准显式协作启动及仅补 CHG-103 的 affects 与变更说明。实施仍须取得当前工作树的 SpecTree 锁。

## 已核实现状
内置清单包含 codex、claude、grok 等，缺少 opencode。进程发现依赖清单匹配，自动接入仅消费 connection.autoConnect 与 descriptorFile，当前未替用户完成端点和已有 session 的发现。ACP 原会话接入使用已有 TCP 字节流，app-server 使用 TCP WebSocket。已有 fixture 和界面通过记录不能证明标准 stdio、Grok WebSocket 或本机共享 daemon 接入。已运行本机四种 CLI 的帮助命令，未启动模型会话。Codex 0.162.1 已导出实验协议 schema，并确认 canAcceptDirectInput 字段存在，不能凭公共文档缺少该字段便移除校验。

## 自动识别
补齐四种具名工具及其实际包装进程识别。通用 Agent 沿用可扩展清单与协议自报身份，通用 ACP 接入不设品牌白名单。辨认执行映像或真实包装入口，用户 prompt、任意文件路径及普通参数中的品牌文本不能冒充 Agent。每个候选继续精确绑定 host、workspace、pane、terminalEpoch 和 process start instance，Remote 保持脱敏。浏览发现不登记成员、不发送 prompt、不触发新会话。

## 自动连接准备
主流程由 Runtime 从已验证本地进程的已公布端点、运行方连接元数据或既有受信连接登记生成候选，用户无需自行编写或寻找 JSON。仅枚举精确候选的本地通信资源，不能扫描任意端口或按 cwd、名称推断 session。多个已有会话时展示协议返回的准确会话供用户选择，无法证明目标终端归属时保持待连接并解释原因。Native 一键连接提交 candidateRef 和明确选择，不接受客户端伪造 PID 或新增会话身份。自动附加继续遵守显式 autoConnect 授权，发现进程不自动等同授予控制权。

## 协议与能力
复用共享会话、投递、审批、历史和组织链路，按协议与传输实现通用连接，不为品牌复制业务状态机。覆盖已存在的 ACP 字节流、ACP WebSocket、已有 app-server 传输与已有 IPC 转发路径。每条路径须核验端点进程启动实例、既有会话、鉴权及可接受控制状态。共享 daemon 不可用相同 cwd 冒充原终端归属，须另有可核验的客户端或会话关联。身份、历史、派发、取消和审批按实际握手及协议事件展示。缺少回放、只读或控制权占用分别说明；手工连接文件保留为高级兼容入口。

## 显式协作启动
用户从终端显式执行 ridge agent 或界面明确的新协作会话动作时，可创建新的协议进程与 session。原生普通 TUI 和已有会话附加继续使用各自原有路径，不因识别或连接动作切换模式。协作终端桥接进程是 Agent 子进程 stdio 的唯一读写持有者，提供同一真实 session 的通用本地附加入口。桥接端点、进程启动实例与会话来源由运行方自动公布至受保护的本地登记位置，正常接入无需用户提供 JSON。

协议、会话、权限和组织状态沿用 Runtime 链路，CLI 仅提供启动与终端交互适配。ACP 使用 initialize、显式 session/new、prompt、cancel 和标准 permission 事件，app-server 使用其对应协议，不将品牌名写入业务分发。原生工具命令与适配器命令保留在可扩展清单。缺少所需适配器或鉴权时就地显示可操作原因，不暗中安装、替换模型或降低权限。

新模式与现有附加动作分别验证，创建失败不留下已登记成员、孤立子进程或被改写的旧组。用户仅断开 Commune 时，终端桥及其 Agent 保持可用；退出协作终端是明确生命周期动作，负责回收它创建的子进程。鉴权、过期来源或用户取消不能触发重建 Agent、重建 session 或重发 prompt。终端与 Remote 对同一会话串行提交，未知结果保留原意图供确认。

## 验收
按工具及传输逐项记录真实 CLI 版本、识别、端点发现、精确 session 选择、身份、历史、派发、审批、取消、失联重连与原组保留。具名四种工具和非内置名称 ACP Agent 均须有真实互操作证据。隔离样本覆盖同名多进程、同目录多会话、PID 复用、端点属于另一进程、旧代际、鉴权拒绝、控制权占用与未知结果。发现、预览和鉴权失败均不能产生模型 prompt、新会话、重复派发或组变更。完整模型轮次及真实网络验收按实际可用环境记录，不能以帮助文本、fixture 或按钮存在宣称全面支持。

## 来源
https://learn.chatgpt.com/docs/app-server
https://learn.chatgpt.com/docs/developer-commands
https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/15-agent-mode.md
https://docs.opencode.ai/docs/acp/
https://agentclientprotocol.com/protocol/v1/transports
https://github.com/agentclientprotocol/claude-agent-acp

## 可重复验收补充
先构建 fake_acp_agent 和 ridge CLI，再运行实际 CLI → PTY → Runtime 的隔离验收。真实 CLI 初始化脚本仅证明初始化与退出，逐项记录失败，不将其提升为完整派发、审批、取消或手机网络验收。ACP 的 --auth-method 由用户显式指定，须属于该 Agent 公布的协议鉴权方法，鉴权拒绝不创建会话。历史视图须区分协作终端观测窗口与 Agent 原生回放，窗口截断须明确显示。

## Windows 精确连接归属兼容
本机受控真实连接已捕获准确四元组及正确非零 PID，但 dwState 分别出现 0 与 LISTEN=2（连接行的 peer port 非零）。归属依据为准确连接四元组、唯一非零 PID、前后相同启动实例与终端祖先关系。调用方持有同一已连接 stream，随后仍须完成协议鉴权及准确 session 附加。TCP 状态字段仅供诊断，连接就绪由实际 stream 和协议握手确认。错误 PID、0 PID、混合归属、过期进程、监听行的空 peer 地址与不同 socket 均拒绝。未知 PID 与缺失连接行仍沿同一 socket 有界观察。诊断只输出本端点状态和 PID，不输出凭据。该证据定位了当前误拒绝条件，未证明其 Windows 更新或驱动根因。


## 显式启动入口整合
CLI 与桌面的显式新建协作动作共用 create_independent_terminal。每次创建独立工作区和分屏，沿用客户端生成的稳定标识及当前 Runtime epoch。重试保留准确标识与启动配置。未知结果在对话框重挂载后继续保留，禁止静默创建另一会话。确认拒绝后可修正参数，仍使用原工作区、分屏与 epoch。已退出或启动失败的协作终端不自动重启，其输出保留供查看。

共同控制台由已打包的 Kernel 可执行程序承载，唯一持有 Agent stdio。清单启动配置保持为数据，ACP/app-server 执行共用实现。Ridge 外部运行 CLI 同样通过 Runtime 创建及控制独立终端，使用既有规范终端客户端。查看端退出保留终端，控制台内显式 /exit 才结束所属子进程树。管道文本换行转换为终端 Enter，CLI 读取真实终端退出状态及退出码，未知退出结果须报错。

桌面启动预览仅限 native trust，保持只读，返回可用清单配置、当前 epoch 和默认工作目录，不启动 Agent 或模型请求。创建成功仅确认终端标识，Agent 启动及鉴权结果显示在该终端。鉴权拒绝、缺少可执行程序或会话创建超时均不产生 Commune 成员，也不自动请求另一会话。验收覆盖准确重试、修改启动参数冲突、native trust、过期 epoch、启动失败、查看端与终端生命周期、实际 CLI 和桌面入口，以及回执丢失后的对话框重挂载。

## 本地监听声明与 IPC 附加
已识别的终端进程可通过清单声明准确的监听子命令和端点参数。Runtime 仅从该进程真实参数识别明确的本地端点，或使用已公布的连接描述。存在多个声明、未知参数形态或无可核验端点时，保持待连接。连接候选只公开发现状态，凭据引用留在 Runtime。

支持 IPv4 回环 TCP 与绝对路径本地 Unix socket。Unix socket 在发送协议数据前读取同一连接的内核对端 PID，并验证声明的进程启动实例及原分屏祖先关系。Windows 兼容成功返回有效 PID 而字节数为零的内核行为。TCP 仍使用原四元组证明。app-server 经 WebSocket 升级复用原驱动，ACP 字节流复用原驱动。会话选择及登记均重复端点和分屏绑定验证。测试覆盖来自另一进程的 socket、无协议数据的拒绝、原会话选择及真实 Windows app-server IPC。共享 daemon 的客户端和会话关联须另有可信证明，不使用相同目录推断归属。

## 本轮验收豁免

用户于 2026-11-16 明确要求跳过 Grok 验证。仅本轮已安装 CLI 初始化检查允许 `--skip-grok`，证据逐项记录 Grok 为 SKIPPED 并说明用户豁免，其真实初始化与完整对话仍未验收。默认命令仍验证全部四种 CLI。其他检查不豁免，不降低协议、鉴权和权限边界。
