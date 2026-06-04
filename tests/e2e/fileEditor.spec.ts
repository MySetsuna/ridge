import { test, expect, type Page } from '@playwright/test';

/**
 * E2E — file editor surface + the `each_key_duplicate` regression guard.
 *
 * Background (2026-06-04): a CRITICAL Svelte crash, `each_key_duplicate`, was
 * traced to a TOCTOU race in `fileEditor.ts::openFile` — opening the SAME file
 * twice (rapid double-click) could append two tabs with the same `path`, and
 * the editor tab strip's keyed `{#each ... (path)}` throws. The store fix
 * re-checks for an existing tab inside its atomic update; this spec locks the
 * behaviour end-to-end in a real browser so a future regression in the
 * component layer (e.g. a non-unique `{#each}` key) is also caught.
 *
 * Layering note: the store-level invariant has a fast, deterministic unit test
 * in `src/lib/stores/fileEditor.test.ts`. This Playwright tier adds the signal
 * the unit test cannot give — that the REAL keyed `{#each}` in
 * `FileEditor.svelte` renders the open tabs without Svelte throwing
 * `each_key_duplicate` at runtime. Tauri IPC is unavailable in pure-browser
 * mode, so `openFile` skips the `read_file_for_editor` disk read (guarded by
 * `isTauri()`) and tabs carry empty content — which is fine: the duplicate-key
 * crash is independent of content.
 */

/**
 * Browser-mode boot helper — identical contract to smoke.spec.ts::bootSpa.
 * The `#brand-loader` splash overlay covers the viewport until it fades out;
 * wait for it to drop from layout before interacting.
 */
async function bootSpa(page: Page): Promise<void> {
  await page.goto('/?e2e=1');
  await page.waitForLoadState('domcontentloaded');
  await page.waitForFunction(
    () => {
      const el = document.getElementById('brand-loader');
      if (!el) return true;
      return getComputedStyle(el).display === 'none';
    },
    null,
    { timeout: 6_000 },
  );
  await warmStoreModule(page);
}

/**
 * Dev-server URL of the file editor store module. The Vite dev server serves
 * project source as ES modules, and `$lib` is aliased to `/src/lib`, so this
 * URL resolves the SAME module-singleton the running app uses. Threaded into
 * `page.evaluate` as a runtime string (NOT a static `import('…literal…')`) so
 * `svelte-check`/tsc don't try to type-resolve a browser-only Vite URL.
 */
const STORE_MODULE_URL = '/src/lib/stores/fileEditor.ts';

/**
 * Force Vite to fully transform + dep-optimize the store module BEFORE any
 * assertion-critical `page.evaluate(import(...))`. On a cold dev server the
 * first import of a module pulling in heavy deps (monaco, marked) can make
 * Vite emit an "optimized dependencies changed, reloading" full-page reload,
 * which destroys the execution context mid-evaluate ("Execution context was
 * destroyed"). Triggering the import here and then re-asserting the SPA is
 * settled absorbs that reload outside the timed assertions, so the real tests
 * run against a stable, warmed context. Tolerant of its own context being
 * torn down by the reload it provokes.
 */
async function warmStoreModule(page: Page): Promise<void> {
  try {
    await page.evaluate(async (moduleUrl) => {
      await import(/* @vite-ignore */ moduleUrl);
    }, STORE_MODULE_URL);
  } catch {
    // The warm-up import itself can be the call whose context Vite destroys
    // with a reload. That's expected — swallow it and re-settle below.
  }
  // Re-confirm the SPA is back and idle after any Vite reload.
  await page.waitForFunction(
    () => {
      const el = document.getElementById('brand-loader');
      if (!el) return true;
      return getComputedStyle(el).display === 'none';
    },
    null,
    { timeout: 8_000 },
  );
  // A second import now hits the warmed graph and proves the context is stable.
  await page.evaluate(async (moduleUrl) => {
    await import(/* @vite-ignore */ moduleUrl);
  }, STORE_MODULE_URL);
}

/** Minimal shape of the store surface the in-page evaluates touch. */
type EvalStore = {
  fileEditorStore: {
    openFile(path: string): Promise<void>;
    subscribe(run: (s: { openFiles: Array<{ path: string }> }) => void): () => void;
  };
};

/**
 * Drive the file editor store directly from inside the page so the editor
 * surface can be exercised deterministically without a Tauri backend or a
 * populated file tree.
 *
 * Returns the number of open tabs whose path === the requested path, so the
 * caller can assert the de-dup invariant.
 */
async function openFileTwiceInPage(page: Page, filePath: string): Promise<number> {
  return page.evaluate(
    async ([moduleUrl, path]) => {
      const mod = (await import(/* @vite-ignore */ moduleUrl)) as EvalStore;
      const store = mod.fileEditorStore;
      // Fire two opens without awaiting the first — mirrors a rapid
      // double-click hitting the TOCTOU window the fix closes.
      const a = store.openFile(path);
      const b = store.openFile(path);
      await Promise.all([a, b]);
      // Read the current state synchronously via the svelte store contract.
      let count = 0;
      const unsub = store.subscribe((s) => {
        count = s.openFiles.filter((f) => f.path === path).length;
      });
      unsub();
      return count;
    },
    [STORE_MODULE_URL, filePath] as const,
  );
}

test.describe('file editor — surface + each_key_duplicate regression guard', () => {
  // The FIRST in-page `import('/src/lib/stores/fileEditor.ts')` triggers Vite's
  // cold on-demand transform of the store's full transitive graph (monaco,
  // marked, …), which can exceed the 30 s default in a cold dev server. The
  // subsequent tests reuse the warmed module. Give every test in this block
  // headroom for the cold path so the suite is deterministic regardless of
  // execution order.
  test.setTimeout(90_000);

  test('opening the same file twice renders one tab and never throws each_key_duplicate', async ({ page }) => {
    // Arrange — capture every console error + uncaught page error; we assert
    // none of them mention the Svelte keyed-each crash.
    const eachKeyDupSeen: string[] = [];
    const record = (text: string) => {
      if (text.includes('each_key_duplicate')) eachKeyDupSeen.push(text);
    };
    page.on('console', (msg) => {
      if (msg.type() === 'error') record(msg.text());
    });
    page.on('pageerror', (err) => record(String(err)));

    await bootSpa(page);

    // Act — open the same path twice through the real store singleton.
    const tabCount = await openFileTwiceInPage(page, '/e2e/fixture/sample.ts');

    // Assert (store invariant) — exactly one tab for the path.
    expect(tabCount).toBe(1);

    // Assert (component invariant) — the editor surface mounted and the keyed
    // `{#each}` rendered without Svelte throwing. Web-first: poll the editor
    // root into existence rather than using a fixed timeout.
    const editor = page.locator('.rg-file-editor').first();
    await expect(editor).toBeVisible({ timeout: 5_000 });
    const tabStrip = page.locator('.rg-editor-tabs-dndzone').first();
    await expect(tabStrip).toBeVisible({ timeout: 5_000 });

    // Assert (regression lock) — no each_key_duplicate fired during the render.
    expect(eachKeyDupSeen).toEqual([]);
  });

  test('the open editor mounts a content surface (monaco host) for the active tab', async ({ page }) => {
    await bootSpa(page);
    await openFileTwiceInPage(page, '/e2e/fixture/content.ts');

    // The active non-image, non-diff tab renders the Monaco host container.
    // We assert it attaches; in browser-only mode Monaco itself may stay empty
    // (no disk content), so we only require the host region to be present —
    // proving the editor body renders rather than collapsing.
    const editor = page.locator('.rg-file-editor').first();
    await expect(editor).toBeVisible({ timeout: 5_000 });
    const monacoHost = page.locator('.rg-monaco-host').first();
    await expect(monacoHost).toBeAttached({ timeout: 5_000 });
  });

  test('opening two different files with the same basename keeps two distinct tabs (no key collision)', async ({ page }) => {
    const eachKeyDupSeen: string[] = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error' && msg.text().includes('each_key_duplicate')) {
        eachKeyDupSeen.push(msg.text());
      }
    });
    page.on('pageerror', (err) => {
      if (String(err).includes('each_key_duplicate')) eachKeyDupSeen.push(String(err));
    });

    await bootSpa(page);

    // Open two distinct paths that share a basename — the tab KEY is the full
    // path, so both must coexist; only the display name collides.
    const total = await page.evaluate(async (moduleUrl) => {
      const mod = (await import(/* @vite-ignore */ moduleUrl)) as EvalStore;
      const store = mod.fileEditorStore;
      await store.openFile('/e2e/a/index.ts');
      await store.openFile('/e2e/b/index.ts');
      let n = 0;
      const unsub = store.subscribe((s) => {
        n = new Set(s.openFiles.map((f) => f.path)).size;
      });
      unsub();
      return n;
    }, STORE_MODULE_URL);

    expect(total).toBe(2); // two unique keys → no each_key_duplicate
    const editor = page.locator('.rg-file-editor').first();
    await expect(editor).toBeVisible({ timeout: 5_000 });
    expect(eachKeyDupSeen).toEqual([]);
  });
});
