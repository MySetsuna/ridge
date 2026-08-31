import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const desktopGate = readFileSync(new URL('../routes/+layout.svelte', import.meta.url), 'utf8');
const mobileGate = readFileSync(new URL('./AuthScreen.svelte', import.meta.url), 'utf8');
const boundVerifyBody =
  'body: `code=${encodeURIComponent(numeric)}&device=${encodeURIComponent(getRemoteDeviceId())}`';

describe('Remote verification device binding', () => {
  it('binds both desktop and mobile verification tokens to the WebSocket device id', () => {
    expect(desktopGate).toContain(
      "const { RemoteConnection, getRemoteDeviceId } = await import('@ridge/remote');",
    );
    expect(desktopGate).toContain(boundVerifyBody);
    expect(mobileGate).toContain(boundVerifyBody);
  });
});
