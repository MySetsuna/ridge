// src/lib/transport/tauriShim/window.ts
//
// Browser stand-in for `@tauri-apps/api/window`. Native window management has no
// meaningful browser equivalent — the web-remote build hides the custom title
// bar / window controls (gated on import.meta.env.RIDGE_WEB_REMOTE in
// +page.svelte) — so these are mostly inert. We still implement onResized /
// isMaximized faithfully because the desktop reads them during layout setup.

import type { UnlistenFn } from './bridge';

class ShimWindow {
  async minimize(): Promise<void> {}
  async maximize(): Promise<void> {
    try {
      await document.documentElement.requestFullscreen?.();
    } catch {
      /* fullscreen denied — ignore */
    }
  }
  async unmaximize(): Promise<void> {
    try {
      if (document.fullscreenElement) await document.exitFullscreen?.();
    } catch {
      /* ignore */
    }
  }
  async toggleMaximize(): Promise<void> {
    if (document.fullscreenElement) {
      await this.unmaximize();
    } else {
      await this.maximize();
    }
  }
  async close(): Promise<void> {}
  async setTitle(_title: string): Promise<void> {}
  async isMaximized(): Promise<boolean> {
    return document.fullscreenElement != null;
  }
  async isFullscreen(): Promise<boolean> {
    return document.fullscreenElement != null;
  }
  async onResized(handler: (event: { payload: { width: number; height: number } }) => void): Promise<UnlistenFn> {
    const fn = () =>
      handler({ payload: { width: window.innerWidth, height: window.innerHeight } });
    window.addEventListener('resize', fn);
    return () => window.removeEventListener('resize', fn);
  }
  async onCloseRequested(_handler: (event: unknown) => void): Promise<UnlistenFn> {
    return () => {};
  }
}

const current = new ShimWindow();

export function getCurrentWindow(): ShimWindow {
  return current;
}

export function getCurrent(): ShimWindow {
  return current;
}
