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
    description: 四处产品版本一致为 0.1.100，独立 crate 版本保持兼容
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
  - L4-CLOUD-TURN-CREDENTIAL-TYPE
  - L4-COMMUNE-LINUX-PROCESS-OBSERVATION
---

# Cloud 连接修复版本发布

产品四处版本同步为 0.1.100，分别为 package.json、Tauri 配置、桌面 Cargo.toml 及 Cargo.lock 的 ridge 包。独立 crate 沿用原版本。已有 v0.1.98、v0.1.99 tag 保持原提交。以同一通过仓库门禁的提交创建 v0.1.100，通过既有 release.yml 生成四平台桌面安装包和 CLI 草稿，保持 releaseDraft: true。通过 publish-remote.yml 发布桌面、移动 Remote，环境变量与秘密只由 GitHub 工作流注入。

此轮汇总已完成的 Kernel TURN Password 类型、早到 ICE、主机重入加密会话重建、移动重连鉴权、Linux 进程观察，以及后续缓存与 CSP、终端视口重绘、工作区元数据、初始化输入和连接前更新入口修复。现有四平台草稿只绑定其自身 tag 提交，新安装包须包含汇总后的源码。全量 npm test、lint、typecheck、版本和 tag 契约以及 SpecTree verify 通过后创建 tag。GitHub 四平台构建均成功并核对草稿资产后，记录实际安装包来源与 SHA。

已有独立 Kernel 的真实公网 ICE/answer、加密鉴权、终端输入、文件打开、刷新、主机重连和强制 TURN/TCP relay 证据继续保留。初始化输入竞态使用真实 Chromium、LAN Kernel 与 PTY 验收，连接前更新使用真实 Workbox SW、隔离静态宿主与只读 Cloud 401 fixture 验收，各自范围明确记录。正式发布前仍需实际 Windows home1 主机升级和公网功能验收，安装版本取可执行文件证据。真实手机、macOS 和真实外部 Agent 端点等待后续提供，浏览器布局与测试端点分别记录。

临时设备在验收结束后撤销、删除，独立 Kernel 正常关闭。既有用户工作会话持续保留。曾受自动审批限制的临时数据保留可逆归档及清理记录。首次资源加载重试、尺寸 RPC 超时及未验收的网络路径继续保留证据。
