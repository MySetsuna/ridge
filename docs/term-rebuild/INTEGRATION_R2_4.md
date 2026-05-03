# Round 2.4 接入步骤

本文档把 round 2.4 产物落到你 ridge 项目里所需的**所有**改动按顺序列出。
预计耗时：30-40 分钟。

> **不可逆性**：完成步骤 1-3 后，wasm 包成为项目依赖；步骤 4-7 是 svelte
> 文件新增 + SplitContainer 一行修改。**xterm 仍然是默认实现**，要打开
> 实验渲染器必须手工把 settings.json 改了。回滚成本：删除 `pkg/`、删除
> 三个新 svelte 文件、还原 SplitContainer 那一行 import。

---

## Step 1: 构建 wasm 包

在 `ridge-term/` 项目根目录：

```bash
# 第一次需要安装 wasm-pack
cargo install wasm-pack

# 构建（release 模式 ≈ 1 分钟）
./build.sh

# 验证
ls pkg/
# 应该看到：
#   ridge_term_bg.wasm   (~150-200KB)
#   ridge_term.js
#   ridge_term.d.ts
#   package.json
```

如果你只想快速试一下、不在乎包体大小，跑 `./build.sh --dev`，编译快 5x。

---

## Step 2: 在 ridge 项目里安装 wasm 包

`ridge` 仓库根目录（即包含 `package.json` 和 `src-tauri/` 的那层）：

```bash
# 假设 ridge-term 仓库在 ../ridge-term
pnpm add file:../ridge-term/pkg
# 或编辑 package.json:
#   "@ridge/term-wasm": "file:../ridge-term/pkg"
# 然后 pnpm install
```

> 如果你的 monorepo 结构不同，把 `file:` 路径改成实际相对路径即可。

---

## Step 3: vite 配置（很可能不需要改）

`vite.config.ts` 大多数情况无需改动 —— wasm-pack `--target web` 输出
的是标准 ESM。但如果你看到 `Failed to fetch dynamically imported module`
之类的错误，加一行：

```ts
// vite.config.ts
export default defineConfig({
	// ... 原有配置 ...
	optimizeDeps: {
		exclude: ['@ridge/term-wasm'],
	},
});
```

`exclude` 是因为 vite 的 dep-pre-bundle 会把 wasm import 错误地当成 ESM
模块走，导致 `init()` 时 fetch wasm 文件失败。`exclude` 让 vite 直接走
浏览器原生 import（这是 wasm-pack 设计的预期路径）。

---

## Step 4: 拷贝新增的 svelte / ts 文件

从 `ridge-integration/` 拷三个文件到 ridge 项目：

```bash
# manager.ts → src/lib/terminal/manager.ts
mkdir -p src/lib/terminal
cp ../ridge-integration/src/lib/terminal/manager.ts src/lib/terminal/manager.ts

# RidgePane.svelte → src/lib/components/RidgePane.svelte
cp ../ridge-integration/src/lib/components/RidgePane.svelte src/lib/components/RidgePane.svelte

# PaneRouter.svelte → src/lib/components/PaneRouter.svelte
cp ../ridge-integration/src/lib/components/PaneRouter.svelte src/lib/components/PaneRouter.svelte
```

---

## Step 5: settings store — 加一个开关字段

`src/lib/stores/settings.ts`：

```diff
 export interface UserSettings {
   /** Claude Code extension surface (rail button + sidebar tab + Bot launcher). */
   claudeExtensionEnabled: boolean;
   ...
   defaultShell: string;
+  /**
+   * 实验性渲染器：用 ridge-term wasm 替换 xterm.js。
+   * 默认 false（用稳定的 xterm）；改 true 后该 pane 改用新引擎。
+   * 切换时该 pane 的 PTY 会被销毁并重建，shell 进程会丢失 —— 这是开发开关，
+   * 不是热切换。round 7 移除 xterm 后此字段也会被删除。
+   */
+  useExperimentalRenderer: boolean;
 }

 const DEFAULTS: UserSettings = {
   claudeExtensionEnabled: true,
   ...
   defaultShell: '',
+  useExperimentalRenderer: false,
 };
```

`load()` 函数里如果有 per-key 类型校验，加一条：

```diff
+      useExperimentalRenderer: typeof obj.useExperimentalRenderer === 'boolean'
+        ? obj.useExperimentalRenderer
+        : DEFAULTS.useExperimentalRenderer,
```

> 我没贴完整 load() 修改是因为你的 settings.ts 注释里说有"per-key type
> narrowing"，每个文件作者的实现可能不同。你按现有模式加一条即可。

---

## Step 6: SplitContainer — 改一行 import

`src/lib/components/SplitContainer.svelte`：

```diff
-import Pane from './Pane.svelte';
+import Pane from './PaneRouter.svelte';
```

只改这一行。`Pane` 这个本地名字不变，所以模板里的 `<Pane ... />` 全部
保持原样。`PaneRouter` 内部对 props 完全透传。

---

## Step 7: 验证 — 默认情况下行为不变

跑一遍 ridge：

```bash
# 在 ridge 项目里
pnpm tauri dev
```

应该一切如常 —— 你刚加的开关默认 `false`，所有 pane 仍然走 xterm 路径。

如果出错，先排查：
- `pnpm install` 后 `node_modules/@ridge/term-wasm/` 是否存在
- 浏览器 DevTools console 是否有 wasm 加载错误（`Failed to fetch …
  ridge_term_bg.wasm`）
- vite dev server 重启了没有（pnpm cache 偶尔会粘旧的）

---

## Step 8: 打开实验渲染器

让用户体验前**先你自己跑通一次**：

打开 ridge 的 DevTools console，跑：

```js
// 假设 ridge 暴露了 settingsStore 到 window，否则用 localStorage：
const s = JSON.parse(localStorage.getItem('ridge-settings') || '{}');
s.useExperimentalRenderer = true;
localStorage.setItem('ridge-settings', JSON.stringify(s));
location.reload();
```

或者在 SettingsPanel 里加一个 toggle —— 长期更可控，但这一轮我没改你的
SettingsPanel（不知道你的 UI 设计语言，怕加得难看）。

刷新后，新建 pane 应该用 ridge-term 渲染。**视觉验证项**：

- [ ] 终端能显示 prompt
- [ ] 输入命令 + 回车，能看到输出（即使颜色不对、字体怪也算通过 — 文字流通了就行）
- [ ] Ctrl+C 能终止 `sleep 100`
- [ ] 颜色：跑 `ls --color=auto`（Unix）或 `ls`（Windows + 启用了 color），看到不同颜色文件名
- [ ] 拖动 splitpanes 边界，终端跟随尺寸变化
- [ ] 滚动：跑 `seq 200`，鼠标滚轮上滚能看到 200..1 的历史
- [ ] Shift+PageUp / Shift+PageDown 翻页
- [ ] 右键菜单：复制（先选一段文字 — 用 `selectAll()` 试，鼠标拖选 round 4 才有）

如果以上**都不通过**，把 DevTools console 的报错和复现步骤贴给我。

---

## 已知不工作的（round 2.4 范围外）

- ❌ **鼠标拖动选择文字** — round 4 manager 全局监听
- ❌ **IME 输入中文** — round 4 IME 重做
- ❌ **Ctrl+F 在 pane 内搜索** — round 4
- ❌ **Ctrl+Click 链接** — round 5
- ❌ **vim/less 退出后 shell 内容恢复** — VT 内核已支持 alt screen，但
  没在浏览器里跑过；如果失败优先怀疑这条
- ⚠️ **多 pane 并存性能** — 这一轮每 pane 一个 canvas，多 pane 时性能
  和 xterm 接近但**没有**预期的"共享 surface 大幅省内存"效果。round
  2.5 才做共享 surface

---

## 回滚

如果接入后想完全回退：

```bash
# 1. SplitContainer.svelte 改回 import './Pane.svelte'
# 2. 删除三个新文件
rm src/lib/components/RidgePane.svelte
rm src/lib/components/PaneRouter.svelte
rm src/lib/terminal/manager.ts
# 3. settings.ts 把 useExperimentalRenderer 字段删除（或留着也无害）
# 4. 卸载 wasm 包
pnpm remove @ridge/term-wasm
```

不需要改后端任何东西 —— 后端 PTY 接口我们这一轮没动。
