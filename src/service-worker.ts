/// <reference types="@sveltejs/kit" />
/// <reference lib="webworker" />
//
// §web-remote service worker. Caches the desktop SPA shell + the multi-MB Monaco
// bundle so, over a weak/remote link, the CODE is served from local cache and
// only DATA crosses the WebSocket. Registered ONLY in the web-remote build
// (src/routes/+layout.svelte); the Tauri build sets kit.serviceWorker.register
// = false, so this file is built but never activated there.
//
// §version-gate: on install/activate, compare the build version (injected by
// Vite via $service-worker) with the version stored in client-side storage.
// If they differ, it means the remote server was updated — nuke ALL client-side
// caches (Cache API, localStorage, sessionStorage, IndexedDB) so we start fresh
// with the new build. This prevents stale tickets, old WASM, or mismatched
// static assets from causing "卡在验证码" or broken UI.

import { build, version } from '$service-worker';

const CACHE = `ridge-web-remote-${version}`;
const HTML_CACHE = `ridge-html-${version}`;
const VERSION_KEY = 'ridge-web-remote-version';
// Precache the content-hashed `_app` bundle (immutable). We intentionally skip
// `files` (favicon, 1.jpg/2.jpg, the nested mobile build) to keep install light.
const PRECACHE = build;

const sw = self as unknown as ServiceWorkerGlobalScope;

// Check if the stored version matches the current build version.
async function checkVersionAndNukeIfNeeded(): Promise<void> {
  try {
    const stored = await sw.clients.matchAll({ includeUncontrolled: true });
    // We can't directly access localStorage from SW, so we use a cache key
    // as a version marker. If the marker cache doesn't exist or has a
    // different version, we nuke everything.
    const caches = await self.caches.keys();
    const versionCache = caches.find(c => c.startsWith('ridge-version-'));
    if (versionCache) {
      const cache = await self.caches.open(versionCache);
      const res = await cache.match('version');
      if (res) {
        const text = await res.text();
        if (text.trim() === version) return; // version matches, no action needed
      }
    }
    // Version mismatch or first run — nuke all caches.
    await Promise.all(caches.map(c => self.caches.delete(c)));
    // Also clear client-side storage via postMessage to all clients.
    const clients = await sw.clients.matchAll({ includeUncontrolled: true });
    clients.forEach(client => client.postMessage({ type: 'CLEAR_STORAGE', version }));
  } catch {
    // If anything fails, continue — the activate handler will also clean up.
  }
}

sw.addEventListener('install', (event) => {
  event.waitUntil(
    checkVersionAndNukeIfNeeded()
      .then(() => caches.open(CACHE))
      .then((cache) => cache.addAll(PRECACHE))
      .then(() => {
        // Store the current version as a marker cache.
        return caches.open(`ridge-version-${version}`).then(c => c.put('version', new Response(version)));
      })
      .then(() => sw.skipWaiting()),
  );
});

sw.addEventListener('activate', (event) => {
  event.waitUntil(
    checkVersionAndNukeIfNeeded()
      .then(() => caches.keys())
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE && k !== HTML_CACHE && !k.startsWith('ridge-version-')).map((k) => caches.delete(k))))
      .then(() => sw.clients.claim()),
  );
});

// Paths that must always hit the network (live data / control plane).
const BYPASS = ['/ws', '/file', '/info', '/verify', '/health', '/status', '/session', '/workspace'];

sw.addEventListener('fetch', (event) => {
  const req = event.request;
  if (req.method !== 'GET') return;
  const url = new URL(req.url);
  if (url.origin !== location.origin) return;
  if (BYPASS.some((p) => url.pathname === p || url.pathname.startsWith(p + '/'))) return;

  // Navigation requests (page reload / address-bar navigations): cache the
  // response on the first successful fetch so subsequent flaky refreshes
  // serve the app shell from cache instead of downloading the HTML.
  if (req.mode === 'navigate') {
    event.respondWith(
      fetch(req)
        .then((res) => {
          if (res.ok) {
            const clone = res.clone();
            void caches.open(HTML_CACHE).then((c) => c.put(req, clone));
          }
          return res;
        })
        .catch(() =>
          caches.match(req).then((cached) => {
            if (cached) return cached;
            // Last resort: the request URL might differ from the cache key
            // (e.g., ? query), so try matching the bare pathname.
            return caches.match(url.pathname).then((fallback) => fallback ?? Response.error());
          }),
        ),
    );
    return;
  }

  // Content-hashed bundle → cache-first (immutable). Everything else →
  // network-first, falling back to cache when offline.
  const immutable = url.pathname.startsWith('/_app/immutable/');
  event.respondWith(
    caches.match(req).then((cached) => {
      if (cached && immutable) return cached;
      return fetch(req)
        .then((res) => {
          if (immutable && res.ok) {
            const clone = res.clone();
            void caches.open(CACHE).then((c) => c.put(req, clone));
          }
          return res;
        })
        .catch(() => cached ?? Response.error());
    }),
  );
});
