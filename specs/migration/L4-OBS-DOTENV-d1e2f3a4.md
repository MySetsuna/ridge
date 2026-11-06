---
id: L4-OBS-DOTENV-d1e2f3a4
level: L4
parent: L3-OBS-VITE-CONFIG-JS-58e6f63d
title: .env
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - .env
  - .env.local
test_targets: []
---

Local env claim.

## CHG-090 映射处置

原父节点 L3-OBS-STATIC-MOBILE-ASSETS-5cc42e7a 属旧生成资产目录，已退役。根环境配置的观察记录归入 Vite 构建配置模块，保留原 code_targets。此次仅修正层级，不读取、复制或提交本地环境文件内容。
