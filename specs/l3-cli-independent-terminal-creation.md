---
id: L3-CLI-INDEPENDENT-TERMINAL-CREATION
level: L3
title: CLI 独立终端创建与重试
status: LOCKED
lifecycle: ACTIVE
parent: L2-UNIFIED-CLIENT-RUNTIME
code_targets:
  - packages/ridge-cli/src/tui/lan_session.rs
  - packages/ridge-cli/src/tui/mod.rs
  - packages/ridge-cli/src/tui/dashboard.rs
  - packages/ridge-cli/src/daemon.rs
  - packages/ridge-kernel/src/host.rs
  - packages/ridge-kernel/src/business.rs
  - packages/ridge-kernel/src/domain.rs
  - packages/ridge-kernel/src/cloud/manager.rs
  - packages/ridge-kernel/src/remote_control.rs
  - packages/ridge-core/src/capability.rs
  - packages/ridge-cli/Cargo.toml
  - Cargo.lock
  - packages/remote/src/shared/cloud/remoteAllowlist.ts
test_targets:
  - packages/ridge-cli/tests/cli_terminal_create_e2e.rs
  - packages/ridge-remote/src/auth.rs
  - packages/remote/src/shared/cloud/remoteAllowlist.test.ts
acceptance_checks:
  - id: cli-terminal-creation-e2e
    command:
      - cargo
      - test
      - --locked
      - -p
      - ridge-cli
      - --test
      - cli_terminal_create_e2e
    description: 真实 Kernel 上四个 CLI 入口的创建、鉴权及上线失败、部分写入回滚、丢包和并发重试验收
  - id: local-grant-expiry
    command:
      - cargo
      - test
      - --locked
      - -p
      - ridge-remote
      - --lib
      - expired_local_grants_reject_replay_without_affecting_live_grants
    description: 共享授权验证器拒绝真实过期记录及其重放，保留同时有效的授权
---

# 统一 CLI 独立终端创建入口

## 范围与依据

落实 `L2-UNIFIED-CLIENT-RUNTIME` 的共同 Host 请求边界。当前本地 TUI（含多会话）、仪表盘、仅 LAN 模式在 CLI 侧直调 Kernel 私有 PTY API。Cloud 模式显式指定 shell 或 CWD 时还会由 CLI 分步创建工作区、保存拓扑和启动 PTY。申请客户端凭据、打开连接或确认对外服务可用失败时，先前的写入可能已生效。

本变更只涉及上述 CLI 启动入口创建初始终端的职责、顺序和失败恢复。docs 导览、历史规范映射清理、其他客户端的分屏契约和对外传输协议各有独立范围。

## 创建契约

- CLI 先发现或启动同一个 Kernel，再以既有本机操作者身份进入共同 Host 请求边界。Host 负责准入、创建及结果；CLI 消费结果，复用 Kernel 已有的工作区拓扑与 PTY 生命周期。LAN 和 Cloud 控制器维持既有权限，本机创建能力不得授予未获授权的远端身份。
- 每个明确的新建操作创建一个独立工作区、首个 pane 和对应 PTY，返回实际身份。不得复用或拆分当前桌面工作区，不得切换其他客户端的选择。保留各入口原有的 shell、CWD、启动 profile 与参数默认值。TUI 多会话中的每次独立创建各有身份。
- 本地 TUI、仪表盘与仅 LAN 模式保留初始终端行为。Cloud 仅在显式提供 shell 或 CWD 时创建终端，未提供时只上线既有 Host；远端文件根仍按现行 root、CWD、进程当前目录的顺序确定。连接失败时不得降级到私有业务写入或在 CLI 中启动本地 PTY。

## 失败与重试

- 操作者身份无效、过期或能力不足，在工作区、拓扑和 PTY 写入前拒绝。TUI 本地授权或握手失败不得留下终端。仅 LAN 监听初始化明确失败、Cloud 上线明确拒绝宿主凭据时，本次启动也不得留下新工作区或 PTY。Cloud 接受异步启动请求只代表开始连接，须依据已认证的上线结果判断本次创建能否完成。该要求约束本次启动的宿主鉴权，后续远端控制器的登录失败不得影响已经提交的终端。
- 提交前明确失败，撤销本次创建产生的部分状态，保留既有工作区、进程和其他客户端连接。只有工作区、拓扑及 PTY 均成功时，才对客户端确认新终端。
- 同一创建操作在超时、断线、响应丢失或并发重放时，最多提交一次；重复请求经过同样的鉴权，成功时返回同一工作区、pane 和 PTY。提交后客户端掉线只解除连接，恢复须使用已提交的身份。旧 Kernel 代际或失效的操作绑定应显式报错，不能静默新建；实现可复用现有身份和代际机制。一次新的显式启动仍创建独立终端，不能按 shell、CWD 或当前活动 pane 猜测重试身份。

## 验收

| 场景 | 通过条件 |
| --- | --- |
| 四条正常入口 | 在隔离的真实 Kernel 上，经 CLI 的 TUI、仪表盘、仅 LAN 和 Cloud 启动路径执行。每次明确创建只新增一个独立工作区、pane 和活 PTY，保留 shell/CWD/profile。第二个有权客户端观察到同一身份，原有客户端的活动工作区保持原选择。覆盖 TUI 多会话和 Cloud 无 shell/CWD 时零新增。 |
| 鉴权与上线失败 | 注入无效或过期的本机授权、能力拒绝和 TUI 握手拒绝，另用占用的显式端口使 LAN 监听失败，用隔离的 Cloud 服务夹具拒绝宿主凭据。逐项断言失败可见、工作区/拓扑/PTY 无新增，预存终端仍可用。远端未授权身份不能调用本机创建。 |
| 部分写入失败 | 在创建过程中注入 PTY 启动失败，断言工作区和拓扑无残留。纠正故障后重试，恰好产生一个终端。 |
| 超时、并发与恢复 | 让创建实际提交后丢失响应，再重放同一操作并覆盖并发重复请求。断言只新增一套身份、返回原 ID、原 PTY 可继续输入和观察输出。对失效授权或旧代际重放，断言拒绝且无新增。另发明确的新建请求，断言创建独立终端。 |

自动验收用拥有独立数据目录的 Kernel、实际 CLI 启动路径和本地 Cloud 鉴权夹具。按场景核对状态差量、身份和真实 PTY 往返。运行结构化验收命令及仓库工程检查后，依 SpecTree 门禁收尾。
