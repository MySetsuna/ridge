# Remote 端 Emoji 字体瘦身设计（系统打底 + 国旗按需兜底）

- 日期：2026-06-10
- 范围：仅 remote（web/移动远控）端字体加载策略。桌面端保持现状。
- 状态：设计已与用户确认 A/B/C 决策点，待复核。

## 1. 背景与问题

remote 端当前为获得"Warp 级"彩色 emoji（含国旗），在 `src/remote/index.html`
里用 `@font-face` 声明了完整的 `NotoColorEmoji.ttf`（**4.8MB**），并在首屏用
`document.fonts.load('64px "Noto Color Emoji"', '🇯🇵😀👍')` 主动预热，导致
**首屏必拉 4.8MB 字体**，remote 请求体积激增。

字体演变历史：

1. 最早 Twemoji（`twemoji-colr-font`，已弃维护）。
2. `0aec7e1` 改用 npm 包 `@fontsource/noto-color-emoji`（11 个 woff2 子集，
   按 `unicode-range` 浏览器按需加载，典型 5–10MB、最坏 25MB）。
3. `9e8f078`（桌面）+ `2708c13`（remote）改为 bundle 单个完整 `NotoColorEmoji.ttf`
   （4.8MB），`1b87efd` 移除 `@fontsource` npm 包。至此退化成"单个完整 TTF 全量加载"，
   连按需子集优势都丢失，且用未压缩的 `truetype` 而非 `woff2`。

### 为何当初要用 Noto 而非系统字体

`src/lib/terminal/fontStack.ts` 与 `2708c13` 提交信息记录了硬约束：

- **Windows 的 Segoe UI Emoji 没有国旗字形**（区域指示符对 🇯🇵🇨🇳…），纯系统字体在
  Windows 上渲染不出国旗。
- 想让桌面 + remote 双端 emoji 风格逐字一致、达到 Warp 水平。

### 不回退陷阱（关键）

Segoe UI Emoji **包含** Regional Indicator 单字母字形（🇯 渲染成带框"J"），所以浏览器
认为系统能渲染这些码点、**不会自动回退**到字体栈后面的国旗字体——这正是当初把整个
Noto 塞到字体栈**最前面**的根因，也因此拖垮了体积。

## 2. 目标 / 非目标

### 目标

- remote 默认**纯系统 emoji 打底**，移除 4.8MB 全量字体的首屏加载。
- 仅对"系统渲染不出的国旗"用一份**极小的国旗子集字体**按需兜底。
- mac/iOS 等系统本就支持国旗的平台：**全程零额外字体请求**。
- Windows/WebView2 等无国旗平台：仅在终端**实际输出国旗**时才下载小字体。

### 非目标

- 不改桌面端。桌面端保持 Noto-first + 本地 bundle 4.8MB（本地资源，非网络请求，
  用户明确要求保留以维持全 emoji 的 Warp 风格）。
- 不追求 remote 端非国旗 emoji 与桌面逐字一致——非国旗 emoji 一律交给系统字体。

## 3. 方案概述：双重 gate

| Gate | 由谁执行 | 作用 |
|---|---|---|
| **能力探测 gate** | 启动时一次 JS 探测（结果缓存） | 系统能渲染国旗（mac/iOS/多数 Android）→ **不注册任何兜底字体，零额外请求**；不能（Windows/WebView2）→ 动态注入国旗 `@font-face` |
| **unicode-range gate** | 浏览器原生 | 即便注册了 `@font-face`，浏览器也只在终端**实际输出国旗码点**时才下载那份小 woff2 |

可行性已确认：remote 栅格化走 `packages/ridge-term/src/render/glyph_rasterizer.rs` 中
**挂载到 `document.body` 的真实 `<canvas>` + `fillText`/`measureText`**（注释明确：
detached canvas / OffscreenCanvas 在 WebView2 上拿不到系统 emoji 与 `@font-face` 字体，
故特意 attach 到 DOM 以继承 document 完整字体链）。因此它走标准 CSS font matching，
`@font-face` + `unicode-range` 完全生效。

## 4. 字体栈与回退机制

通过"国旗字体排到系统 emoji 之前 + `unicode-range` 限定它只管国旗码点"破解不回退陷阱：

```
不支持国旗的系统（注入后）：  TEXT_MONO, 'Flag Emoji', 'Apple Color Emoji','Segoe UI Emoji', monospace
支持国旗的系统（不注入）：    TEXT_MONO, 'Apple Color Emoji','Segoe UI Emoji', monospace
```

- 国旗码点 🇯🇵 → TEXT_MONO 无 → `Flag Emoji`（range 命中）→ 彩色旗 ✅
- 其他 emoji 😀 → `Flag Emoji`（range 不命中，跳过）→ 系统 Segoe/Apple ✅
- 文本/CJK → TEXT_MONO ✅

`@font-face` 声明（仅在能力探测判定不支持时动态注入）：

```css
@font-face {
  font-family: 'Flag Emoji';
  src: url('/fonts/flags.woff2') format('woff2');
  unicode-range: U+1F1E6-1F1FF, U+1F3F4, U+E0020-E007F;
  font-display: swap;
}
```

`unicode-range` 含义：
- `U+1F1E6-1F1FF`：26 个区域指示符（组合成标准双字母国旗）。
- `U+1F3F4` + `U+E0020-E007F`：subdivision flags 的基字符（waving black flag）与
  tag characters，用于英格兰🏴󠁧󠁢󠁥󠁮󠁧󠁿/苏格兰🏴󠁧󠁢󠁳󠁣󠁴󠁿/威尔士🏴󠁧󠁢󠁷󠁬󠁳󠁿（**决策点 A：包含**）。

桌面端 `DEFAULT_TERM_FONT` 维持 Noto-first 不变，因此 `fontStack.ts` 需**分出 remote 变体**
（新增 `REMOTE_TERM_FONT` / 让 `withEmojiFallback` 接受策略参数），两端不再共用同一条栈。

## 5. 组件设计

### 5.1 国旗子集字体 `flags.woff2`

- 从现有 `NotoColorEmoji.ttf` subset 出 Regional Indicator 国旗（~270 glyph）
  + 3 个 subdivision flags（决策点 A），输出 woff2。
- 放置 `src/remote/public/fonts/flags.woff2`，由 LAN host 经 `/fonts` 托管。
- **删除** `src/remote/public/fonts/NotoColorEmoji.ttf`（4.8MB）。

### 5.2 subset 构建脚本

- 新增可复现脚本（`scripts/build-flag-font.mjs` 或等价），核心命令：
  ```
  pyftsubset NotoColorEmoji.ttf \
    --unicodes=U+1F1E6-1F1FF,U+1F3F4,U+E0020-E007F \
    --flavor=woff2 --output-file=flags.woff2 \
    --layout-features='*' --no-ignore-missing-unicodes
  ```
- 依赖 fonttools（Python）。脚本与依赖在文档/README 注明，便于重建。
- 源字体可临时从 git 历史或 npm `@fontsource/noto-color-emoji` 取得（不再常驻仓库）。

### 5.3 能力探测模块（remote TS 新增）

- 在 `document.body` 创建一个隐藏 `<canvas>`（与栅格器同款 attach 策略，保证字体链一致）。
- 画 🇯🇵（真实国家码）与对照 🇿🇿（无效国家码，所有系统都退化成两个字母）。
- 像素/宽度对比：两者渲染**相同** → 系统不支持国旗；**不同** → 系统支持。
  - 备选判据：`measureText` 宽度（单 glyph 国旗 advance ≈ 1em；两个字母 ≈ 2 字符宽）。
- 判定**不支持** → 动态注入 §4 的 `@font-face`，并让 remote 字体栈采用含 `'Flag Emoji'` 的变体。
- 判定**支持** → 不注入任何字体，字体栈用纯系统变体。

### 5.4 缓存（决策点 B）

- 探测结果写 `localStorage`（键如 `ridge.flagEmojiSupport`）。
- 带**失效指纹**：以 `navigator.userAgent`（或 UA + app 版本）哈希为 key 的一部分，
  环境变化（换设备/系统升级）时缓存自然失效、重新探测。
- 命中缓存则跳过 canvas 探测，直接决定是否注入。

### 5.5 清理项

- 删 `src/remote/index.html` 的全量 `@font-face`（Noto 4.8MB）与
  `document.fonts.load('…🇯🇵😀👍')` 预热脚本。
- `src/remote/lib/terminalController.ts` 改用 remote 字体栈变体。
- 删 `src/remote/public/fonts/NotoColorEmoji.ttf`。

## 6. 数据流

```
启动 → 能力探测（一次，读缓存）
        ├ 支持国旗 → 不注入任何字体；栈 = 纯系统变体
        └ 不支持   → 注入国旗 @font-face（带 unicode-range）；栈 = 含 'Flag Emoji' 变体
终端输出 → 栅格器 fillText(remote 栈)
        ├ 普通 emoji → 命中系统 Segoe/Apple
        └ 国旗码点   → 命中 'Flag Emoji'（此刻浏览器才下载 flags.woff2）→ 彩色旗
```

## 7. 体积预算与退出条件（决策点 C）

- 国旗 woff2 预期 ~200–500KB（COLRv1 矢量，~270 glyph）。
- **红线：若 subset 后 > 800KB**，回退评估：换 bitmap/CBDT 子集、或接受、或放弃国旗兜底
  退回纯系统。该验证作为实现**第一步**执行，体积不达标即在动其余代码前回到设计。

## 8. 测试与验证

- subset 后体积核对（对照 §7 红线）。
- Windows/WebView2 实测：普通 emoji 走系统、国旗（含 subdivision）彩色显示、
  **首屏 Network 无字体请求**、输出国旗时才出现一次 `flags.woff2` 请求。
- mac/iOS 实测：国旗走系统、全程零字体请求。
- 能力探测单测：mock canvas 像素 → 验证支持/不支持两分支与缓存失效逻辑。
- 回归：桌面端不受影响（fontStack 桌面变体未变）。

## 9. 风险

- **能力探测误判**：某些 Android/WebView 的国旗字形不全。判据需对比 🇿🇿 对照而非绝对像素，
  并保留 `RIDGE_DIAG` 式诊断开关。
- **subdivision flags 的 tag 匹配**：依赖浏览器把整个 grapheme cluster 交给能渲染基字符
  `U+1F3F4` 的字体；`unicode-range` 已含基字符与 tag range，实测核对苏格兰/威尔士。
- **体积超红线**：见 §7 退出条件。
