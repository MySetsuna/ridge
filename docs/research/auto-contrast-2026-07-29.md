# 自动对比度研究（2026-07-29）

关联：`REQ-AUTO-CONTRAST-RESEARCH-01`

## 裁决

不做全局运行时“看见低对比即改色”。先用静态 token 配对检查覆盖 Ridge 自有 UI；保留浏览器
`forced-colors` 默认行为。渐变、图片、半透明叠层及终端 ANSI 面若须持续采样方能判断，则依停机
条件 deferred。

## 标准基线

- WCAG 2.2 AA：普通文字 `>= 4.5:1`，大字 `>= 3:1`。当前可作发布闸。
  来源：<https://www.w3.org/TR/WCAG22/#contrast-minimum>
- WCAG 3.0 仍为 Working Draft；不可把其中演进中的感知对比方法当稳定发布阈值。
  来源：<https://www.w3.org/TR/wcag-3.0/>
- forced colors 由 UA/OS 强制用户色板，可给文字加 backplate；作者应默认
  `forced-color-adjust: auto`，仅在自身完整处理可读性时 opt out。
  来源：<https://www.w3.org/TR/css-color-adjust-1/#forced>

## 候选对账

| 方案 | 正确性 | 运行成本 | 反例 / 风险 | 裁决 |
| --- | --- | --- | --- | --- |
| 静态 token 前景/背景配对 lint | 对纯色、自有组件可判定 | 构建期 O(token pairs) | 不知实际叠层、图片像素 | **采用为最小方案** |
| WCAG 2.2 相对亮度阈值 | 稳定、可复核 | 构建期常数 | 不能表达所有感知差异 | **发布闸** |
| WCAG 3 / APCA 类感知模型 | 可作影子报告 | 构建期常数 | 草案会变；双阈值易冲突 | **只观察，不阻断** |
| `forced-colors` + system colors | 尊重用户/OS 偏好 | 浏览器原生 | 品牌色、图示会变化 | **默认允许；做视觉 fixture** |
| `getComputedStyle` 逐元素判色 | 仅知 computed 色 | 每次样式/主题变化 O(elements) | 透明祖先、混合模式、伪元素误判 | **拒绝** |
| 截图/Canvas 逐像素采样 | 可近似最终像素 | O(pixels × updates) + 回读 | 跨域图片、GPU stall、动画抖动 | **触发停机条件** |
| 终端 ANSI 自动改色 | 破坏应用语义 | 每 cell/帧 | TUI 状态色、真彩色失真 | **不做** |

## 可证伪最小原型

输入仅为显式 token 配对，不扫描 DOM：

```text
contrastPairs = [
  ["text-primary", "surface-primary", 4.5],
  ["text-muted", "surface-primary", 4.5],
  ["text-on-accent", "accent", 4.5],
  ["focus-ring", "surface-primary", 3.0],
]
```

对每对颜色按 WCAG 2.2 计算 `(Llighter + 0.05) / (Ldarker + 0.05)`；不足即构建失败，并输出
token 名、实测比值、阈值。以下 fixture 可直接裁决实现：

| 前景 | 背景 | 期望 |
| --- | --- | --- |
| `#000000` | `#ffffff` | `21:1`，通过 |
| `#777777` | `#ffffff` | 约 `4.48:1`，普通文字失败 |
| `rgba(..., 0.5)` | 任意 | 拒绝输入；须先显式合成至纯色 |
| 渐变 / 图片 | 任意 | 标为 unsupported；不得猜一个背景色 |

## 最小落地顺序

1. 盘点自有 UI 的语义 token 配对；删去无消费者 token。
2. 加纯函数 + 上表 fixture；WCAG 2.2 AA 为唯一阻断阈值。
3. 加 light/dark/forced-colors 三张隔离视觉 fixture；不改终端 ANSI。
4. 若主要低对比问题仍来自图片、渐变或透明动态叠层，停止自动化；逐组件显式修色。

## 状态

研究完成；全局动态判色 **deferred / 不实现**。后续若授权产品化，只授权“静态 token lint +
forced-colors fixture”这一减法方案。

