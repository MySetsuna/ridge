/** Build an idempotent teardown for transport/component listeners. */
export function onceCleanup(stops: readonly (() => unknown)[]): () => void {
  let finished = false;
  return () => {
    if (finished) return;
    finished = true;
    for (const stop of stops) {
      try {
        stop();
      } catch {
        // One broken unsubscribe must not strand the remaining listeners.
      }
    }
  };
}
