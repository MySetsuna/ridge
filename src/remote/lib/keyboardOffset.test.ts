import { describe, it, expect } from 'vitest';
import { terminalViewportHeightPx } from './keyboardOffset';

const MIN = 48;

/** 一台「布局 800、底部 Tab 56、终端从 y=100 起」的典型手机。 */
const phone = (visualHeightPx: number, visualOffsetTopPx = 0) => ({
  layoutHeightPx: 800,
  visualHeightPx,
  visualOffsetTopPx,
  containerTopPx: 100,
  chromeBelowPx: 56,
  minHeightPx: MIN,
});

describe('terminalViewportHeightPx（手机软键盘下的终端高度自适配）', () => {
  it('键盘收起返回 null —— 高度交还 flex:1', () => {
    expect(terminalViewportHeightPx(phone(800))).toBeNull();
  });

  it('退化输入（视觉视口反而更高）也返回 null，不产生诡异高度', () => {
    expect(terminalViewportHeightPx(phone(900))).toBeNull();
    expect(terminalViewportHeightPx(phone(Number.NaN))).toBeNull();
  });

  it('键盘弹出后容器收到「可见区底边 - 顶边 - 底部 chrome」', () => {
    // 键盘 300 → 视觉视口 500；可见底边 500；500 - 100 - 56 = 344。
    expect(terminalViewportHeightPx(phone(500))).toBe(344);
  });

  it('页面被上推时（offsetTop>0）按可见区底边算，而不是按视觉高', () => {
    // iOS 聚焦输入框会把页面上推：视觉视口高 500，但整体下移 40。
    expect(terminalViewportHeightPx(phone(500, 40))).toBe(384);
  });

  it('键盘越高，终端越矮 —— 单调，且永不越过键盘顶边', () => {
    let prev = Number.POSITIVE_INFINITY;
    for (const kb of [200, 260, 320, 420]) {
      const h = terminalViewportHeightPx(phone(800 - kb))!;
      expect(h).toBeLessThan(prev);
      // 容器底边 = 100 + h + 56，必须落在可见区底边（800-kb）之内。
      expect(100 + h + 56).toBeLessThanOrEqual(800 - kb);
      prev = h;
    }
  });

  it('极端挤压时钳到下限，绝不给出 0/负高（否则 refit 会算出 0 行）', () => {
    // 键盘吃掉 780 → 可见区只剩 20，可用高为负。
    expect(terminalViewportHeightPx(phone(20))).toBe(MIN);
  });

  it('是纯函数：同输入恒同输出（旧实现正是因为读了动画中的 transform 才发散）', () => {
    const input = phone(517, 13);
    expect(terminalViewportHeightPx({ ...input })).toBe(terminalViewportHeightPx(input));
  });
});
