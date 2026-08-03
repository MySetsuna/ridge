import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const appSource = readFileSync(new URL('./App.svelte', import.meta.url), 'utf8');
const providerSource = readFileSync(new URL('./lib/sidebarProvider.ts', import.meta.url), 'utf8');

describe('Remote Query policy', () => {
  it('does not retry failed remote queries behind the transport reconnect loop', () => {
    expect(appSource).toContain('retry: false');
    expect(appSource).toContain('refetchOnReconnect: false');
  });

  it('keeps explicit refresh on the same Query key while bypassing fresh data', () => {
    expect(providerSource).toContain('const runFresh = <T>');
    expect(providerSource).toContain('run(key, query, observerSignal, 0)');
    expect(providerSource).toContain('refreshDir');
    expect(providerSource).toContain('refreshGit');
  });
});
