/// <reference types="@sveltejs/kit" />
/// <reference lib="webworker" />
//
// §web-remote service worker. Caches the desktop SPA shell + the multi-MB Monaco
// bundle so, over a weak/remote link, the CODE is served from local cache and
// only DATA crosses the WebSocket. Registered ONLY in the web-remote build
// (src/routes/+layout.svelte); the Tauri build sets kit.serviceWorker.register
// = false, so this file is built but never activated there.

import { build, version } from '$service-worker';

const CACHE = `ridge-web-remote-${version}`;
const HTML_CACHE = `ridge-html-${version}`;
// Precache the content-hashed `_app` bundle (immutable). We intentionally skip
// `files` (favicon, 1.jpg/2.jpg, the nested mobile build) to keep install light.
const PRECACHE = build;

const sw = self as unknown as ServiceWorkerGlobalScope;

sw.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(PRECACHE))
      .then(() => sw.skipWaiting()),
  );
});

sw.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE && k !== HTML_CACHE).map((k) => caches.delete(k))))
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
