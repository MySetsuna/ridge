/**
 * RAF-based throttling for resize mousemove events.
 * Prevents excessive updates by batching pointer moves into animation frames.
 */

let rafId: number | null = null;
let pendingPointer: { x: number; y: number } | null = null;

export function throttledUpdateResize(pointer: { x: number; y: number }, callback: (pointer: { x: number; y: number }) => void) {
  pendingPointer = pointer;
  if (rafId !== null) return;
  rafId = requestAnimationFrame(() => {
    rafId = null;
    if (pendingPointer) {
      callback(pendingPointer);
      pendingPointer = null;
    }
  });
}

export function cancelThrottledResize() {
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
  pendingPointer = null;
}