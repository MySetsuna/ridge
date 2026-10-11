/** Share one lazy request; a rejected request never becomes a permanent cache. */
export function retryableLazyImport<T>(importModule: () => Promise<T>): () => Promise<T> {
  let pending: Promise<T> | null = null;
  let failedUrl: URL | null = null;
  let failedCssUrl: URL | null = null;
  let attempt = 0;
  return () => {
    if (!pending) {
      pending = Promise.resolve().then(async () => {
        if (failedCssUrl && typeof document !== 'undefined') {
          const url = new URL(failedCssUrl);
          url.searchParams.set('ridge-retry', String(++attempt));
          await new Promise<void>((resolve, reject) => {
            const stylesheet = document.createElement('link');
            stylesheet.rel = 'stylesheet';
            stylesheet.href = url.href;
            const timer = setTimeout(() => failed(), 10_000);
            const failed = () => {
              clearTimeout(timer);
              stylesheet.remove();
              reject(new Error('stylesheet retry failed'));
            };
            stylesheet.onload = () => { clearTimeout(timer); resolve(); };
            stylesheet.onerror = failed;
            document.head.appendChild(stylesheet);
          });
          failedCssUrl = null;
        }
        if (failedUrl) {
          const url = new URL(failedUrl);
          url.searchParams.set('ridge-retry', String(++attempt));
          // A failed ESM module may remain in the browser module map. Retry
          // only the same-origin module URL reported by its failed loader.
          const module = await import(/* @vite-ignore */ url.href);
          if (!('default' in module)) throw new Error('module export changed; reload required');
          return module as T;
        }
        return importModule();
      }).catch(error => {
        if (typeof location !== 'undefined' && error instanceof TypeError) {
          const match = /(?:Failed to fetch dynamically imported module|error loading dynamically imported module):\s*(https?:\/\/\S+)$/i.exec(error.message);
          if (match) {
            try {
              const url = new URL(match[1]);
              if (url.origin === location.origin && /\.(?:js|svelte)$/.test(url.pathname)) failedUrl = url;
            } catch { /* retain the original loader for unsupported errors */ }
          }
        }
        if (typeof location !== 'undefined' && error instanceof Error) {
          const match = /^Unable to preload CSS for (\S+)$/.exec(error.message);
          if (match) {
            try {
              const url = new URL(match[1], location.href);
              if (url.origin === location.origin && url.pathname.endsWith('.css')) failedCssUrl = url;
            } catch { /* never load an unrecognized stylesheet target */ }
          }
        }
        pending = null;
        throw error;
      });
    }
    return pending;
  };
}
