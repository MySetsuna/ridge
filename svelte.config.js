// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
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
