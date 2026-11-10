---
id: L4-MONACO-TRUSTED-STYLES
level: L4
title: 严格 CSP 下的编辑器动态样式与生命周期
status: LOCKED
lifecycle: ACTIVE
parent: L3-OBS-SRC-LIB-MONACO-c2b128b2
code_targets:
  - src/lib/monaco/trustedThemeStyles.ts
  - src/lib/monaco/ridgeTheme.ts
  - scripts/monaco-trusted-styles.mjs
  - vite.config.js
test_targets:
  - scripts/monaco-trusted-styles.test.mjs
  - scripts/monaco-csp-e2e.mjs
  - src/lib/monaco/ridgeTheme.test.ts
  - scripts/app-csp.test.mjs
acceptance_checks:
  - id: monaco-csp-browser
    command:
      - node
      - scripts/monaco-csp-e2e.mjs
    description: 真实 Chromium 在严格 CSP 下编辑、diff、主题切换及样式生命周期；未授权样式继续拒绝
---

# 严格 CSP 下的编辑器动态样式与生命周期

本契约约束 Native 与 Remote 文件编辑器由受信 Monaco 代码生成的主题和动态规则，不允许把文件内容作为样式执行。现有 CSP 的 script、style 与网络来源约束保持有效，不增加 unsafe-inline 授权或静态 nonce。

Monaco 动态样式经 CSSOM 生效，不能先插入被 CSP 拦截的标签再用观察器补救。主题变更、动态规则增删和跨文档样式克隆保持可用；ShadowRoot 样式归属原根；释放仅清理自身样式，不得清空其他 adoptedStyleSheets。源代码钩子不匹配时构建明确失败，不发布未受约束的回退。

验收以实际 Monaco 与真实 Chromium 为准：可输入修改模型，diff 得到结果，主题颜色变化，正常路径零 CSP 违规；ShadowRoot 与克隆样式更新及释放正确，既有样式保留。另注入未授权样式证明 CSP 继续拒绝。未修补库的负对照须复现 CSP 失败；现有 Ridge 主题与应用 CSP 回归保持通过。
