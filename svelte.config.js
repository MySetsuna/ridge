// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

// Browser Remote has one physical artifact root. The complete desktop SPA lives
// under `remote-dist/desktop`; the lightweight touch SPA lives under
// `remote-dist/mobile`. No base path: both are served at an isolated origin root
// after the host/cloud selects a UI shape.
const WEB_REMOTE = !!process.env.RIDGE_WEB_REMOTE;

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: "index.html",
      ...(WEB_REMOTE
        ? { pages: "remote-dist/desktop", assets: "remote-dist/desktop" }
        : {}),
    }),
    // §web-remote: the service worker (src/service-worker.ts) is built for both
    // targets but only REGISTERED in the web-remote boot (+layout.svelte). The
    // Tauri webview loads from disk and wants no SW intercepting its fetches.
    serviceWorker: { register: false },
    alias: {
      "@components": "src/lib/components",
      "@stores": "src/lib/stores",
      "@types": "src/lib/types",
      // 统一远控包：裸导入 @ridge/remote 走桶(传输层公共面);深子路径
      // @ridge/remote/shared/cloud/* 直取模块(cloud 各模块导出名重叠、且有命名
      // 空间导入,不并入 flat 桶——见设计 R3.2)。SvelteKit 由此 alias 生成
      // @ridge/remote 与 @ridge/remote/* 两条 tsconfig paths(svelte-check 用)。
      "@ridge/remote": "packages/remote/src",
      // Transport-agnostic UI shared with the plain-Svelte remote app
      // (see vite.mobile.config.js for the mirror alias).
      "@shared": "src/shared",
    },
  },
};

export default config;
