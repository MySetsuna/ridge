---
id: L3-ARCHITECTURE-DOCUMENTATION
level: L3
title: 架构导览、权威契约引用与历史材料边界
status: LOCKED
lifecycle: ACTIVE
parent: L2-UNIFIED-CLIENT-RUNTIME
code_targets:
  - README.md
  - docs/architecture/README.md
  - docs/architecture/00-overview.md
  - docs/architecture/10-kernel.md
  - docs/architecture/20-rtp1-wire.md
  - docs/architecture/30-remote-lifecycle.md
  - docs/architecture/40-cli.md
  - docs/architecture/50-desktop-tauri.md
  - docs/architecture/60-data-flow.md
  - docs/architecture/70-runtime-epoch.md
  - docs/architecture/80-security.md
  - docs/architecture/90-performance.md
  - docs/archive/CHG-089/architecture/README.md
  - docs/archive/CHG-089/architecture/00-overview.md
  - docs/archive/CHG-089/architecture/10-kernel.md
  - docs/archive/CHG-089/architecture/20-rtp1-wire.md
  - docs/archive/CHG-089/architecture/30-remote-lifecycle.md
  - docs/archive/CHG-089/architecture/40-cli.md
  - docs/archive/CHG-089/architecture/50-desktop-tauri.md
  - docs/archive/CHG-089/architecture/60-data-flow.md
  - docs/archive/CHG-089/architecture/70-runtime-epoch.md
  - docs/archive/CHG-089/architecture/80-security.md
  - docs/archive/CHG-089/architecture/90-performance.md
  - docs/architecture/RIDGE-RUNTIME-FOUNDATION-FINAL.md
  - docs/architecture/specs/L2-PROTO-001.md
  - docs/architecture/specs/L2-REMOTE-001.md
  - docs/architecture/specs/L2-TERM-001.md
  - docs/architecture/specs/L2-PERF-001.md
  - docs/architecture/notes/README.md
test_targets:
  - scripts/check-architecture-docs.mjs
acceptance_checks:
  - id: architecture-docs
    command:
      - node
      - scripts/check-architecture-docs.mjs
    description: 检查当前导览内部链接、必需契约与入口引用，历史副本明确标识且旧导览按原文归档
---

# 架构文档投影

## 范围与依据

按用户要求独立整理 docs，承接 CHG-088 的现行 CLI 创建契约，并引用统一 Runtime、Remote 终端、Commune 的已批准规范。根 specs/ 与 changes/ 经 SpecTree 管理，docs 解释源码现状、流程和历史，不自行定义协议。

## 当前导览

根 README 与架构目录说明独立 Kernel 的业务及 PTY 所有权、最小 Tauri OS 展示层、CLI/TUI、本地授权 WebSocket、LAN 与 Cloud 适配、共同业务分发、权威终端语义生产和展示消费。逐项链接真实源码、测试和根目录规范，区分当前实现、兼容面与历史设计。CLI 四入口、独立终端、鉴权失败、未知提交结果与同身份重试均引用已批准契约。Commune 说明 Host 全局归属及持久化/临时状态边界。

## 历史材料

改写前的十一篇架构导览原文保存在 docs/archive/CHG-089/architecture/。Foundation 报告、历史验收和 docs/architecture/specs/ 副本明确标识历史身份并指向现行导览或根规范。保留历史图片作为历史证据，当前导览不再用其表达新拓扑。

## 验收

当前导览无断裂的本地 Markdown 链接，各章节有具体实现或测试依据。当前入口不得将 docs 副本宣称为权威，不得把旧 RTP1 adapter、Tauri Channel 或旧桌面模块写作现役业务主链。归档十一篇正文与提案前 Git 版本逐字节一致。文档检查器验证链接、权威引用、历史标识和归档完整性，人工核对流程与源码，执行 npm test、npm run lint、npm run typecheck 和 SpecTree verify/complete。协议字段、产品行为与规范映射清理由各自变更控制。
