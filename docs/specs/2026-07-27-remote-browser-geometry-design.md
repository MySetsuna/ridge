# 桌面浏览器 Remote 几何同源设计

状态：`APPROVED`（`REQ-REMOTE-03`）

## 根因

桌面浏览器启用 `TerminalManager.sharedRemoteMode`。`_recomputeViewport` 按远端
kernel grid 在 pane content box 内居中 letterbox；`cellFromEvent` 却只减 CSS
padding，未减居中偏移。故画面原点与输入原点不同。尺寸上另有
`getBoundingClientRect()`、`clientWidth`、live padding 多个读取点，claim 的
pixelWidth/pixelHeight 可与 renderer/grid 所用内容区不同。

## 单一几何快照

新增无 DOM 纯函数：

```ts
type PaneGeometry = {
  rows: number;
  cols: number;
  contentXCss: number;
  contentYCss: number;
  contentWidthCss: number;
  contentHeightCss: number;
  gridXCss: number;
  gridYCss: number;
  gridWidthCss: number;
  gridHeightCss: number;
  viewportDevice: { x: number; y: number; w: number; h: number };
};
```

输入仅含 container rect、host rect、四边实际 padding、cell CSS 尺寸、DPR、
可选远端 kernel rows/cols。输出同时供 renderer viewport、rows/cols、claim pixel
尺寸、pointer/touch→cell。DOM 只在 manager 边界读取一次。

坐标规则：

- `clientX/clientY`、DOM rect、padding、cellW/cellH 均为 CSS px，不乘 DPR。
- device viewport 只在最终输出用 DPR 量化。
- shared grid 小于 content box 时居中；pointer 减 `gridXCss/gridYCss`。
- shared grid 大于 content box 时裁剪，但 cell origin 不因 clamp 漂移。
- 边界点击 clamp 至现有 kernel grid；零尺寸返回 null。

## 生命周期

- mount/unpark、workspace switch、split/sidebar/window ResizeObserver、DPR drift、
  reconnect 皆经 manager 重算。
- 复用既有 ResizeObserver 每帧合并、500ms trailing fit 与 pointerup flush；不加
  新轮询/throttle。
- 浏览器 shared remote 的首次 attach、稳定布局及 reconnect 发送由同一快照导出的
  rows/cols/content pixel size；LAN/public 仅 transport 不同。

## 验收

- 纯函数覆盖 DPR `1/1.25/1.5/2`、非对称 padding、分数 rect/cell、居中/裁剪、
  四角与边界点击。
- manager 测证明 renderer viewport 与 pointer origin 同快照。
- LAN/public browser fixture 断言初挂、split、sidebar、window/DPR、reconnect 后
  host rows/cols 与本地 grid 同源；resize 次数有界、末尺寸必达。

