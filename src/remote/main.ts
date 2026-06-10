/// <reference types="vite-plugin-pwa/client" />
import { mount } from 'svelte';
import App from './App.svelte';
import { registerSW } from 'virtual:pwa-register';

const app = mount(App, { target: document.getElementById('app')! });

// PWA service worker: precache the app shell for offline use and apply new
// releases automatically. The SW is generated in 'prompt' mode (see
// vite.remote.config.js) so the new version *waits* instead of reloading
// immediately; we drive the swap ourselves — silently, but only when the tab
// is backgrounded — so a release never reloads mid-keystroke. The WS layer
// reconnects with the saved token on reload, so the session resumes
// transparently. (No-op in dev: devOptions.enabled is false.)
let applyUpdate: ((reloadPage?: boolean) => Promise<void>) | undefined;
let updateReady = false;

function flushUpdateWhenHidden() {
  if (updateReady && applyUpdate && document.visibilityState === 'hidden') {
    updateReady = false;
    void applyUpdate(true); // SKIP_WAITING + reload into the new build
  }
}

applyUpdate = registerSW({
  immediate: true,
  onNeedRefresh() {
    updateReady = true;
    // If already backgrounded apply now; otherwise wait for the next time the
    // user switches away (frequent on mobile) — never interrupt the foreground.
    flushUpdateWhenHidden();
  },
});

document.addEventListener('visibilitychange', flushUpdateWhenHidden);

// §version-gate: listen for CLEAR_STORAGE message from SW (sent on version
// mismatch). This clears all client-side storage to ensure a clean slate with
// the new build. Then reload to re-authenticate cleanly.
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.addEventListener('message', (event) => {
    if (event.data?.type === 'CLEAR_STORAGE') {
      console.log('[remote] Clearing client storage due to version mismatch:', event.data.version);
      try { localStorage.clear(); } catch {}
      try { sessionStorage.clear(); } catch {}
      // IndexedDB clearing is async; best-effort for known DBs.
      try {
        indexedDB.databases?.().then(dbs => {
          for (const db of dbs) {
            if (db.name) indexedDB.deleteDatabase(db.name);
          }
        });
      } catch {}
      // Reload to start fresh with new build.
      window.location.reload();
    }
  });
}

export default app;
