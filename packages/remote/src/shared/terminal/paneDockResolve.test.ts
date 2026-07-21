import { describe, it, expect } from 'vitest';
import { regionAtPoint, passedDragThreshold, resolveDockTarget } from './paneDockResolve';

function rectEl(x: number, y: number, w: number, h: number) {
  return {
    getBoundingClientRect: () =>
      ({ left: x, top: y, width: w, height: h, right: x + w, bottom: y + h, x, y, toJSON() {} }) as DOMRect,
  };
}

describe('regionAtPoint', () => {
  const el = rectEl(0, 0, 100, 100); // m=0.18 → 边带 18px
  it('左带 → left', () => expect(regionAtPoint(5, 50, el)).toBe('left'));
  it('右带 → right', () => expect(regionAtPoint(95, 50, el)).toBe('right'));
  it('上带 → top', () => expect(regionAtPoint(50, 5, el)).toBe('top'));
  it('下带 → bottom', () => expect(regionAtPoint(50, 95, el)).toBe('bottom'));
  it('中心 → center', () => expect(regionAtPoint(50, 50, el)).toBe('center'));
});

describe('passedDragThreshold', () => {
  it('小位移不算拖拽', () => expect(passedDragThreshold(0, 0, 2, 2)).toBe(false));
  it('超阈值算拖拽', () => expect(passedDragThreshold(0, 0, 10, 0)).toBe(true));
});

describe('resolveDockTarget', () => {
  // mock：closest 命中一个带 data-pane-id 的 100×100 wrapper（paneId=null → closest 返回 null）。
  function elWithPane(paneId: string | null) {
    const wrapper =
      paneId == null
        ? null
        : {
            getAttribute: (k: string) => (k === 'data-pane-id' ? paneId : null),
            getBoundingClientRect: () =>
              ({ left: 0, top: 0, width: 100, height: 100, right: 100, bottom: 100, x: 0, y: 0, toJSON() {} }) as DOMRect,
          };
    return { closest: (_sel: string) => wrapper } as unknown as Element;
  }

  it('命中目标 pane → {paneId, region}', () => {
    expect(resolveDockTarget(elWithPane('pane-b'), 'pane-a', 5, 50)).toEqual({ paneId: 'pane-b', region: 'left' });
  });
  it('命中源 pane 自身 → null', () => {
    expect(resolveDockTarget(elWithPane('pane-a'), 'pane-a', 50, 50)).toBeNull();
  });
  it('无 pane 容器 → null', () => {
    expect(resolveDockTarget(elWithPane(null), 'pane-a', 50, 50)).toBeNull();
  });
  it('el 为 null → null', () => {
    expect(resolveDockTarget(null, 'pane-a', 50, 50)).toBeNull();
  });
});
