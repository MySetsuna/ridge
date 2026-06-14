# Web-Remote 连接缓慢 + 陈旧包 修复设计

日期：2026-06-13
分支：develop
关联提交：ce6e679（emoji 字体瘦身，删 4.8MB Noto）、756c4ae（/ws 三段埋点）

## 背景 / 症状

用户报告 **desktop browser remote（浏览器复用完整桌面 UI over remote）连接十分缓慢**，并补充三条观察：

1. 公网远控仍在请求 `fonts/NotoColorEmoji.ttf`（已在 ce6e679 删除的 4.8MB 字体）。
2. 希望对 web-remote 做按需加载 / 代码拆分。
3. 公网远控未像 LAN 那样做手机端/桌面端 UA 分叉；LAN 远控文案应更明确（"局域网远控"）。

## 诊断（已用硬数据确认）

### 根因①：陈旧包（字体没被清理）
- `git show ce6e679` 证实 `static/fonts/NotoColorEmoji.ttf`（4,991,984B）已从源码删除。
- 根目录新构建 `web-remote-dist/`（20MB）已无 .ttf —— 源码层干净。
- 但 `target/release/{web-remote-dist,static/remote}/fonts/NotoColorEmoji.ttf` + `tauri-codegen-assets/*.ttf` 仍残留 —— **vite/tauri 产物拷贝是增量的，源码删除的文件不会从输出目录/codegen 被清除**。
- 公网（ridge-cloud）最后部署 969f557（2026-06-07）早于字体瘦身（2026-06-13）→ 云端 bundle 陈旧。
- 结论：`tauri build` 的 `beforeBuildCommand` 确实会重建三套产物，但 ①旧产物残留会被打进安装包/部署包；②云端是独立部署，必须单独 rebuild + redeploy。

### 根因②：服务端零压缩
- `src-tauri/src/remote/server.rs` 的 `assets_handler`/`spa_fallback_handler`/`serve_index` 全是裸 `fs::read` 直发，router 无 `CompressionLayer`。20MB 产物全程不压缩传输。

### 根因③：首屏 eager 包过大（Monaco 没懒加载）
- `web-remote-dist/index.html` 的 modulepreload 含 `CiLVb0ke.js = 4.2MB`，指纹检测 monaco 命中 153 处 → Monaco 核心被打进首屏关键路径。
- 拖入链：`src/routes/+page.svelte`（顶层 `import FileEditor` + 5 个 `?worker` + MonacoEnvironment）、`src/lib/utils/markdown.ts`（顶层 `import * as monaco`，仅 `highlightCodeBlocks` 用）。
- 对比：mermaid（2.7MB）已是懒加载（`markdown.ts` 内 `await import('mermaid')`）。

## 修复方案（按优先级）

### P0-A 服务端压缩（src-tauri）
- `Cargo.toml` 加 `tower-http = { version = "0.6", features = ["compression-gzip","compression-br"] }`（lock 已有 0.6.8 + brotli）。
- `server.rs` router 加 `.layer(CompressionLayer::new())`。
- 预期：20MB→~5MB 传输；immutable 资产浏览器只下一次。

### P0-B 构建产物清理（杜绝陈旧字体复发）
- `build:desktop-web` / `build:remote` 输出目录已被各自的构建器清空（SvelteKit adapter-static / `emptyOutDir:true`）。残留只在 `target/` 暂存层。
- 加一步显式 prune：`scripts/build-desktop-web.mjs` 构建后删除 `web-remote-dist` 下任何 `*.ttf`（Noto 类大字体不该出现在 web-remote）。作为兜底，确保即便上游回归也不再打进包。
- 云端：rebuild + redeploy（用户动作，文档说明）。

### P1 Monaco 懒加载（按需加载 / 代码拆分）
- `markdown.ts`：删顶层 `import * as monaco`，改为 `highlightCodeBlocks` 内 `await import('monaco-editor')`（照搬现有 `loadMermaid` 范式）。
- `+page.svelte`：`FileEditor` 改动态 `import()`，仅在打开文件时加载；MonacoEnvironment + 5 个 worker 的设置随之延迟到首次需要编辑器时。
- 预期：首屏 eager chunk 从 ~4.2MB 降到 ~1MB 级；Monaco 仅在打开文件/渲染 md 代码高亮时拉取。
- 风险：桌面端编辑器回归 —— 需 rebuild + 验证打开文件、md 预览高亮、diff 仍工作。

### P2-A 公网 UA 分叉（ridge-cloud，独立仓库）
- 调查 ridge-cloud 如何 serve SPA；复用 LAN 的 UA 判定（mobile 关键词）分发 mobile vs desktop bundle。

### P2-B LAN 文案明确化（wind）
- 远控面板文案标注"局域网远控"，与公网远控区分。

## 验证
- 代码拆分：`pnpm build:desktop-web` 后重测 `web-remote-dist/index.html` 的 eager chunk 体积。
- 压缩：`cargo check`；运行时经 `pnpm tauri:dev:cdp` 自助验证 Content-Encoding（见 [[feedback_self_verify_via_cdp]]）。
- 桌面回归：rebuild 后验证编辑器/终端（需用户重启 ridge，见共享 tree 注意事项 [[feedback_shared_tree_git_amend]]）。

## 实施结果（2026-06-13 完成）

### wind（已实现 + 验证）
- **压缩层**：`Cargo.toml` 加 `tower-http compression-gzip/br` + `server.rs` router `.layer(CompressionLayer::new())`。`cargo check --bin ridge` 通过。
- **代码拆分**：根因是 `manualChunks` 强制 monaco/mermaid 成块，rollup 把 Vite `__vitePreload` helper co-locate 进去 → 根 layout/app entry 静态 `import{_}` 把整块拖成 eager。移除 monaco+mermaid 两条 manualChunks 规则（二者已纯动态引用）+ markdown.ts 懒 import monaco + FileEditor 懒挂载（`+page.svelte` 首次打开文件才 `import()`）。**首屏 eager JS：4.42MB → 152KB（−96.5%）**；monaco(3.77MB)/mermaid(2.7MB) 变懒块；monaco-editor.css(142KB) 也移出 eager。桌面 `build/` eager 同步降到 153KB（冷启动连带受益）。`pnpm build` + `build:desktop-web` 均通过，svelte-check 0/0，markdown 30 测试绿。
- **构建产物清理**：`scripts/prune-stale-fonts.mjs`（按 >1MB 体积清字体，绝不误伤 codicon/flags），接进 `build-desktop-web.mjs`（beforeBuildCommand、cargo bundle 前跑）；已清掉 6 份残留 4.76MB NotoColorEmoji.ttf。
- **LAN 文案**：`remote.title/enabledLabel/startLabel` 中英改为"局域网远控 / LAN remote"。
- **UA 分叉复用 SSOT**：新增 `ridge_remote::ua`（`is_mobile_ua`/`prefer_desktop_ui` + 3 单测绿），`server.rs::wants_desktop_ui` 改调它。

### ridge-cloud（已实现 + cargo check/test 绿，待部署）
- **根因确认**：仓库**提交了** `desktop-app/fonts/NotoColorEmoji.ttf`(4,991,984B) + `CiLVb0ke.js`(4.2MB 分包前) —— 公网仍请求字体 + 首屏慢的直接来源（desktop-app 是 pre-diet 25MB 旧快照，最后同步 6384c5d）。
- **UA 分叉**：`src/ua.rs`（镜像 `ridge_remote::ua` 的 SSOT，附同步说明 + 未来发 git crate 真复用 TODO）+ `config.mobile_app_dir`(env `MOBILE_APP_DIR`，缺省 `mobile-app`) + `router.rs::spa_fallback` 租户分支按 UA 分流（`?ui=` 可覆盖；mobile-app 缺失优雅回退桌面 → 未部署 mobile 包时行为不变）。cargo check + 3 ua 测试绿。

## 待用户部署的动作（hard-to-reverse，未自动执行）
1. **公网去字体 + 带分包**：把新构建的 wind `web-remote-dist/*` 同步覆盖 ridge-cloud `desktop-app/`，提交 + redeploy → 杀掉 4.99MB 字体 + 首屏 4.4MB→152KB。
2. **激活公网手机分叉**：把 wind `static/remote/*` 拷进 ridge-cloud `mobile-app/`、Dockerfile 加 `COPY mobile-app ./mobile-app`、redeploy。
3. **本机安装包**：`pnpm tauri build`（prune 守卫会自动清残留字体；压缩+分包随产物带上）。
4. 三仓共享 tree，commit/push 前核对 HEAD（见 [[feedback_shared_tree_git_amend]]）。
