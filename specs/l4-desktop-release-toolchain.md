---
id: L4-DESKTOP-RELEASE-TOOLCHAIN
level: L4
title: 桌面发布使用已验证的固定 Rust 工具链
status: DRAFT
lifecycle: PLANNED
parent: L3-REMOTE-INTERACTION-RELIABILITY
code_targets:
  - .github/workflows/release.yml
test_targets:
  - .github/workflows/release.yml
---

桌面 release 工作流的测试门禁与四平台构建均使用同一固定 Rust 1.99.1 工具链，不能随 stable 自动漂移。现有 Tauri 2.6.2 依赖构建脚本的旧 OUT_DIR 布局，此固定版本来自已经成功生成四平台 0.1.100 安装包的实际 CI。保留所有源码、版本、协议、Native compile、Runtime 回归与 sidecar 校验门禁，不通过移除 Cargo crate 目录或忽略错误规避冲突。

代码来自明确的不可变 tag。通过 workflow_dispatch 从带修复的工作流引用重新构建 v0.1.101 时，仍 checkout v0.1.101 的原始源码，不改 tag、不夹带未审核产品代码。执行记录须区分工作流提交与产品源码提交。既有正式发布前 Windows home1 完整验收条件保持不变。

验收读取实际 CI 工具链版本与构建路径；Linux Tauri compile check 不再把 crate 目录误作 sidecar，完整前置测试及 Windows、Linux、macOS arm64/x64 四平台全部成功，资产大小、来源与校验值分别核对。仓库 npm test、lint、typecheck、SpecTree verify 与 complete 仍执行。依赖库将来支持新的 Cargo 布局时，工具链升级另行验证，不在本轮升级无关依赖。
