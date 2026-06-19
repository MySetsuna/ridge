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

// SW registration must be robust: a transient failure (flaky network on first
// load, the server still warming up, a race against an in-flight old SW) used to
// fail *silently* — the PWA then never installed and offline/precache never
// kicked in. We now log every outcome and retry registration a few times with
// backoff before giving up. registerSW returns the updater synchronously, so the
// retry only re-drives the underlying navigator.serviceWorker.register.
const SW_URL = '/sw.js';
const SW_SCOPE = '/'; // must match `scope` in vite.remote.config.js manifest
const MAX_SW_RETRIES = 3;
const SW_RETRY_BASE_MS = 2000;

function manualRegisterWithRetry(attempt = 0): void {
  if (!('serviceWorker' in navigator)) return;
  navigator.serviceWorker
    .register(SW_URL, { scope: SW_SCOPE })
    .then((reg) => {
      console.log('[remote] SW registered (retry path), scope:', reg.scope);
    })
    .catch((err) => {
      console.error(`[remote] SW register failed (attempt ${attempt + 1}/${MAX_SW_RETRIES + 1}):`, err);
      if (attempt < MAX_SW_RETRIES) {
        const delay = SW_RETRY_BASE_MS * 2 ** attempt; // 2s, 4s, 8s
        setTimeout(() => manualRegisterWithRetry(attempt + 1), delay);
      } else {
        console.error('[remote] SW registration giving up; PWA/offline disabled this session.');
      }
    });
}

applyUpdate = registerSW({
  immediate: true,
  onNeedRefresh() {
    updateReady = true;
    // If already backgrounded apply now; otherwise wait for the next time the
    // user switches away (frequent on mobile) — never interrupt the foreground.
    flushUpdateWhenHidden();
  },
  onRegisteredSW(swUrl, registration) {
    // Success path: confirm the SW took control at the expected scope. A scope
    // mismatch (SW served from a sub-path / wrong Service-Worker-Allowed) means
    // it can't intercept the app shell — surface it loudly instead of failing
    // quietly with a non-installable PWA.
    if (registration && !registration.scope.endsWith(SW_SCOPE)) {
      console.warn('[remote] SW scope mismatch — expected', SW_SCOPE, 'got', registration.scope);
    } else {
      console.log('[remote] SW registered at', swUrl, registration ? `(scope ${registration.scope})` : '');
    }
  },
  onRegisterError(error) {
    // The plugin's own register failed — fall back to a manual register loop so
    // a transient first-load failure doesn't permanently disable the PWA.
    console.error('[remote] vite-plugin-pwa SW register error:', error);
    manualRegisterWithRetry();
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
