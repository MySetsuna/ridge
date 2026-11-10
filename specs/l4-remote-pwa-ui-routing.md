---
id: L4-REMOTE-PWA-UI-ROUTING
level: L4
title: 移动 PWA 外壳缓存与跨 UI 资源归属
status: LOCKED
lifecycle: ACTIVE
parent: L3-OBS-VITE-REMOTE-CONFIG-JS-9b1a8a99
code_targets:
  - vite.remote.config.js
test_targets:
  - scripts/verify-remote-pwa-build.mjs
acceptance_checks:
  - id: mobile-pwa-build
    command:
      - npm
      - run
      - build:remote:mobile
    description: 验证实际生成 SW 的移动外壳键、缓存迁移与现有 PWA 门禁
---

# 移动 PWA 外壳缓存与跨 UI 资源归属

移动 PWA 预缓存外壳时须显式请求 ui=mobile，离线导航回退使用同一预缓存键。Worker 的默认 User-Agent 或预览浏览器形态不能将桌面 HTML 写成移动外壳。既有污染的预缓存须迁入新的移动代码缓存命名空间，登录凭据、控制方身份及草稿仍遵守既有持久化契约。显式桌面 UI 导航继续请求服务端。

服务端静态资源按实际构建文件归属读取，UA 与 UI query 负责选择文档；移动 Worker 请求移动资源及移动浏览器选择桌面文档时，其脚本、样式与 WASM 均取得正确文件和 MIME。缺失 bundle 返回 404，普通 SPA 路由仍可回退对应外壳。

验收检查实际编译 SW 的移动 HTML 请求键、回退键与新缓存命名空间；公网用默认桌面 UA 的移动预览复现旧失败，再覆盖移动首连、Worker 激活后刷新、真实终端输入、显式 UI 切换与离线外壳。测试须区分桌面浏览器模拟与实体手机。运行 npm test、lint、typecheck、移动构建及 SpecTree verify。
