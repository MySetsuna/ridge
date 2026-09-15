// Device-class 判定：决定手机端 SPA 是否需要露虚拟键盘栏。
//
// 目标：所有设备默认拿到手机端 SPA（含电脑浏览器）。电脑带实体键盘，
// 显式操作时用浏览器自身的软键盘 / 物理键盘即可，**不**应该再叠一层
// 虚拟键盘栏（占位 + 误触）。手机/平板没实体键盘时，虚拟键盘栏才是
// 唯一输入手段，必须保留。
//
// 信号：`(pointer: fine)` 是精确指针（鼠标 / 触控板），`(hover: hover)`
// 是 hover 能力。两者同时为真基本可以认为「这是个能接鼠标/键盘的桌面/笔电
// 浏览器」；触屏主导设备两项均 false（手机/平板裸触屏）。
//
// 兼容：旧浏览器没有 `(hover: hover)` / `(pointer: fine)` 时 `matchMedia`
// 会返回 `null.matches=false`，函数自然落到 false（视为触屏）。
// SSR 安全：`typeof window === 'undefined'` 时直接返回 false。
export interface DeviceClass {
  /** 真指针 + 真 hover → 视为带实体键盘；不必叠虚拟键盘。 */
  hasRealKeyboard: boolean;
}

export function readDeviceClass(): DeviceClass {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return { hasRealKeyboard: false };
  }
  const pointerFine = window.matchMedia('(pointer: fine)').matches;
  const hoverHover = window.matchMedia('(hover: hover)').matches;
  return { hasRealKeyboard: pointerFine && hoverHover };
}

/** 订阅设备类变化（如外接键盘 / 平板接鼠键、折叠屏切换）。
 * 返回反订阅函数。 */
export function watchDeviceClass(cb: (cls: DeviceClass) => void): () => void {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return () => {};
  }
  const queries = ['(pointer: fine)', '(hover: hover)'];
  const mqls = queries.map((q) => window.matchMedia(q));
  const fire = () => cb(readDeviceClass());
  for (const mql of mqls) {
    if (typeof mql.addEventListener === 'function') {
      mql.addEventListener('change', fire);
    } else if (typeof (mql as MediaQueryList & {
      addListener?: (cb: (ev: MediaQueryListEvent) => void) => void;
    }).addListener === 'function') {
      // Safari < 14 fallback
      (mql as MediaQueryList & {
        addListener: (cb: (ev: MediaQueryListEvent) => void) => void;
      }).addListener(fire);
    }
  }
  return () => {
    for (const mql of mqls) {
      if (typeof mql.removeEventListener === 'function') {
        mql.removeEventListener('change', fire);
      } else if (typeof (mql as MediaQueryList & {
        removeListener?: (cb: (ev: MediaQueryListEvent) => void) => void;
      }).removeListener === 'function') {
        (mql as MediaQueryList & {
          removeListener: (cb: (ev: MediaQueryListEvent) => void) => void;
        }).removeListener(fire);
      }
    }
  };
}
