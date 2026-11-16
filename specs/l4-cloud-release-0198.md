---
id: L4-CLOUD-RELEASE-0198
level: L4
title: Cloud 连接修复版本发布与验收
status: LOCKED
lifecycle: ACTIVE
parent: L3-REMOTE-INTERACTION-RELIABILITY
code_targets:
  - package.json
  - Cargo.lock
  - src-tauri/Cargo.toml
  - src-tauri/tauri.conf.json
test_targets:
  - scripts/check-release-version.mjs
  - scripts/check-release-version.test.mjs
acceptance_checks:
  - id: release-version
    command:
      - node
      - scripts/check-release-version.mjs
    description: 四处产品版本一致为 0.1.101，独立 crate 版本保持兼容
  - id: release-version-regression
    command:
      - pnpm
      - exec
      - vitest
      - run
      - scripts/check-release-version.test.mjs
    description: 版本及 tag 契约拒绝缺失或不一致
depends_on:
  - L4-CLOUD-HOST-REJOIN-SESSION
  - L4-CLOUD-MOBILE-REAUTH-GATE
  - L4-CLOUD-PANE-SUBSCRIPTION-CONTINUITY
  - L4-CLOUD-TURN-CREDENTIAL-TYPE
  - L4-COMMUNE-LINUX-PROCESS-OBSERVATION
  - L4-COMMUNE-MOBILE-GROUP-NAVIGATION
  - L4-COMMUNE-SHARED-DAEMON-BINDING
  - L4-DESKTOP-TERMINAL-NAVIGATION-FOCUS
  - L4-REMOTE-ACTIVE-PANE-CWD
---

# Cloud 连接修复版本发布

产品四处版本同步为 0.1.101，分别为 package.json、Tauri 配置、桌面 Cargo.toml 及 Cargo.lock 的 ridge 包，独立 crate 沿用原版本。已有 v0.1.98、v0.1.99、v0.1.100 tag 保持原提交。以同一通过仓库门禁的提交创建 v0.1.101，通过既有 release.yml 生成四平台桌面安装包和 CLI 草稿，保持 releaseDraft: true。通过 publish-remote.yml 发布桌面、移动 Remote，环境变量与秘密只由 GitHub 工作流注入。

本轮安装包包含既有连接与缓存、CSP、终端视口、工作区元数据与初始化输入修复，及 CHG-122/123 的剪贴板构建隔离、强制协议源码漂移门禁，CHG-124 的当前 pane 目录作用域、Cloud 订阅连续性和移动 Commune 导航，CHG-125/128 的真实 Agent 接入与可信共享 daemon 绑定，CHG-126/127 的跨平台 cwd 验收和当前架构，以及 CHG-129 显式工作区导航后的键盘焦点恢复。全量 npm test、lint、typecheck、版本和 tag 契约以及 SpecTree verify 通过后创建 tag。GitHub 四平台构建均成功并核对草稿资产后，记录实际安装包来源、大小与 SHA。

已有真实独立 Kernel 的公网 answer、加密鉴权、终端输入、文件打开、刷新、主机重连和 TURN/TCP relay 证据继续保留。每组新证据须明确源码、构建、平台与环境，隔离服务、模拟手机和本地受控模型不能替代真实外部系统。正式发布前仍需实际 Windows home1 主机升级和公网完整功能验收，安装版本取可执行文件证据。测试开启非 breakaway 模式只证明功能，生产关窗和强制退出后的 Kernel、PTY 保活另行验收。物理手机、真实外部 Agent、UDP TURN 与弱网性能仍按可用环境推进；Grok 保留用户豁免 SKIPPED。

临时设备在验收结束后撤销、删除；独立测试 Kernel 正常关闭。保留用户工作会话、既有工作区与凭据。首次加载重试、尺寸超时及未验收网络路径继续保留证据。
