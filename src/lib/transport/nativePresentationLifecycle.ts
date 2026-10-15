import { listen } from '@tauri-apps/api/event';
import { TerminalManager } from '@ridge/remote/shared/terminal/manager';

/** Window visibility belongs to the presentation, independent of Runtime attachment. */
export function startNativePresentationLifecycle(): () => void {
  let alive = true;
  const stops: Array<() => void> = [];
  const handlers = {
    'ridge://memory-reclaim': () => {
      if (alive) TerminalManager.tryInstance()?.reclaimTerminalMemory({
        documentHidden: true, forceHeapPressure: true,
      });
    },
    'ridge://memory-restore': () => {
      if (alive) TerminalManager.tryInstance()?.restoreTerminalMemory();
    },
  };
  for (const [event, handler] of Object.entries(handlers)) {
    void listen(event, handler).then(stop => {
      if (alive) stops.push(stop);
      else stop();
    }).catch(() => { /* A closing WebView may reject registration. */ });
  }
  return () => { alive = false; stops.splice(0).forEach(stop => stop()); };
}
