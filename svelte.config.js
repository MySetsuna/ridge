// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

// §web-remote: the desktop-in-browser build (RIDGE_WEB_REMOTE=1) emits to
// `web-remote-dist/` — a sibling of the Tauri `build/` output, deliberately
// OUTSIDE `static/` so adapter-static doesn't recursively copy the 1.4M static
// dir (which itself holds the mobile build) into the output. The host's remote
// server serves this dir to desktop browsers (UA-forked). No base path: the
// SvelteKit app keeps its `/_app/*` asset prefix, which never collides with the
// mobile build's `/assets/*`.
const WEB_REMOTE = !!process.env.RIDGE_WEB_REMOTE;

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: "index.html",
      ...(WEB_REMOTE ? { pages: "web-remote-dist", assets: "web-remote-dist" } : {}),
    }),
    // §web-remote: the service worker (src/service-worker.ts) is built for both
    // targets but only REGISTERED in the web-remote boot (+layout.svelte). The
    // Tauri webview loads from disk and wants no SW intercepting its fetches.
    serviceWorker: { register: false },
    alias: {
      "@components": "src/lib/components",
      "@stores": "src/lib/stores",
      "@types": "src/lib/types",
      // Transport-agnostic UI shared with the plain-Svelte remote app
      // (see vite.mobile.config.js for the mirror alias).
      "@shared": "src/shared",
    },
  },
};

export default config;
