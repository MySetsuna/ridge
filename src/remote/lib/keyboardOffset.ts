// Pure geometry for the mobile soft-keyboard terminal fit.
//
// §kb-height（iter-63，用户实测 0.1.4「唤起键盘后可见区错位、被压到底部」）:
// 之前的做法是保持容器**满布局视口高**，再用 `translateY` 把光标行顶到键盘上方。
// 问题在于手机（尤其 iOS）弹键盘只缩**视觉**视口（`visualViewport.height`），
// 布局视口 `innerHeight` 不变——于是 `flex:1` 的容器下半截天然被键盘压住，位移
// 只能救回光标那一行，其余内容仍在键盘后面；位移量一旦与实际 chrome 高对不上，
// 整个可见区就是错位的。
//
// 现在改为**收容器的高**：让终端宿主的高度跟随视觉视口，网格随之 refit（PTY 也
// 跟着 resize），整屏内容都落在键盘上方。没有 transform，也就没有「位移 vs 布局」
// 两套坐标打架，GPU scissor 与容器 bbox 天然一致。
//
// 这里只放**纯几何**，与 DOM 无关，便于单测钉死：输出只由入参决定，重复计算收敛。

export interface TerminalViewportInput {
  /** 布局视口高（`window.innerHeight`）。键盘弹出时 iOS 不变、部分安卓会变小。 */
  layoutHeightPx: number;
  /** 视觉视口高（`visualViewport.height`）。键盘弹出即变小——这是唯一可靠信号。 */
  visualHeightPx: number;
  /** 视觉视口相对布局视口的下移（`visualViewport.offsetTop`）。页面被上推时非 0。 */
  visualOffsetTopPx: number;
  /** 终端容器**顶边**在布局视口中的 y（`getBoundingClientRect().top`）。 */
  containerTopPx: number;
  /** 容器**下方**要让出的固定 chrome 高（底部 Tab 栏 + 安全区）。
   *  在键盘收起时测得（那时无任何自适应高度介入），故是稳定常量。 */
  chromeBelowPx: number;
  /** 高度下限：再挤也不能把终端压没（否则 refit 会算出 0 行）。 */
  minHeightPx: number;
}

/**
 * 键盘弹出时终端容器应有的高度（CSS px）。键盘收起返回 `null`
 * —— 调用方据此**撤掉**内联高度，把布局交还给 `flex:1`。
 *
 * 推导（全部落在布局视口坐标系）：
 *   可见区底边 = visualOffsetTop + visualHeight
 *   容器可用高 = 可见区底边 - containerTop - chromeBelow
 */
export function terminalViewportHeightPx({
  layoutHeightPx,
  visualHeightPx,
  visualOffsetTopPx,
  containerTopPx,
  chromeBelowPx,
  minHeightPx,
}: TerminalViewportInput): number | null {
  const keyboardPx = layoutHeightPx - visualHeightPx;
  if (!(keyboardPx > 0)) return null; // 键盘收起（含 NaN/退化输入）→ 交还 flex
  const visibleBottom = visualOffsetTopPx + visualHeightPx;
  const usable = visibleBottom - containerTopPx - chromeBelowPx;
  return Math.max(minHeightPx, Math.round(usable));
}
