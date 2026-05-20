/**
 * Restore Settings.parserBackend to 'rust' (the product default).
 *
 * The test build (`src-tauri/target/release/ridge.exe`) and the
 * installed host (`C:\Program Files\ridge\ridge.exe`) share the
 * `com.tauri-app.ridge` Tauri identifier, which means they share the
 * same WebView2 user-data-dir + localStorage. The wdio e2e suite
 * (parserBackend.switch.spec, parserBackend.wasm.spec, perf wasm
 * round) leaves localStorage with `parserBackend = 'wasm'`. When
 * the host ridge is restarted, it would read that on next boot.
 *
 * This one-shot spec launches the test ridge.exe (which mounts the
 * same localStorage), writes 'rust' back, and exits. The running
 * host instance still has its in-memory copy of the OLD value until
 * its next restart — but next time it boots, it'll see 'rust'.
 *
 * Run via:  pnpm e2e:reset
 *
 * Lives under tests/e2e-utils/ (NOT tests/e2e-shell/) so the default
 * e2e:shell suite doesn't pick it up and mutate state unexpectedly.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady } from '../e2e-shell/helpers';

describe('reset Settings.parserBackend', () => {
  before(async () => {
    await waitForAppReady();
  });

  it('writes parserBackend=rust to localStorage', async () => {
    const result = await browser.execute(() => {
      const raw = localStorage.getItem('ridge-settings');
      const obj = raw ? JSON.parse(raw) : {};
      const before = obj.parserBackend ?? '(unset → default rust)';
      obj.parserBackend = 'rust';
      localStorage.setItem('ridge-settings', JSON.stringify(obj));
      const verify = JSON.parse(localStorage.getItem('ridge-settings') || '{}').parserBackend;
      return { before, after: verify };
    });
    // eslint-disable-next-line no-console
    console.log(`[reset] parserBackend: ${result.before} -> ${result.after}`);
    expect(result.after).toBe('rust');
  });
});
