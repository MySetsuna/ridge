// src/lib/actions/scrollOverlay.ts
//
// Toggles `.wf-scrolling` on the target element for a short window after any
// `scroll` event so the overlay scrollbar style (defined in app.css under
// `.wf-scroll-overlay.wf-scrolling`) can flash the thumb visible during active
// scroll and fade out when idle. Pair with the `wf-scroll-overlay` class on the
// same element — the class provides the hover-to-widen styling; this action
// provides the scroll-triggered visibility.

const IDLE_MS = 800;

export function scrollOverlay(el: HTMLElement) {
  let timer: ReturnType<typeof setTimeout> | null = null;

  function onScroll() {
    el.classList.add('wf-scrolling');
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      el.classList.remove('wf-scrolling');
      timer = null;
    }, IDLE_MS);
  }

  el.addEventListener('scroll', onScroll, { passive: true });

  return {
    destroy() {
      el.removeEventListener('scroll', onScroll);
      if (timer) clearTimeout(timer);
      el.classList.remove('wf-scrolling');
    },
  };
}
