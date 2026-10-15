import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import config from '../vite.remote.config.js';

describe('current-source Remote acceptance startup', () => {
  it('uses the embedded kernel and existing CDP runtime directory', () => {
    const launcher = readFileSync(new URL('./tauri-dev-cdp.mjs', import.meta.url), 'utf8');
    expect(launcher).toContain("['build', '-p', 'ridge']");
    expect(launcher).toContain('RIDGE_KERNEL_HOST_BINARY = ridgeKernelCopy');
    expect(launcher).toContain("buildKernelSidecar(['--debug']).destination");
    expect(launcher).toContain("path.join(userDataDir, 'dev-kernel')");
    expect(launcher).toContain("RIDGE_REMOTE_UI_ROOT = path.join(root, 'remote-dist')");
    expect(launcher).toContain("identifier: cdpApplicationIdentifier(userDataDir)");
    expect(launcher).toContain("RIDGE_CDP_NATIVE_DATA_DIR = path.join(userDataDir, 'native-data')");
    expect(launcher).not.toContain("'--bin', 'rdg'");
    expect(launcher).not.toContain('RIDGE_RDG_BINARY');
    expect(launcher).toContain('applyKernelBreakawayPolicy(process.env);');
    expect(launcher).not.toContain('applyKernelBreakawayPolicy(process.env, true)');
    expect(launcher).not.toContain("'.iteration'");
  });

  it('proxies HTTP authentication and WebSocket control to the TLS host', () => {
    const proxy = config.server?.proxy;
    expect(proxy?.['/health']).toMatchObject({ target: 'https://127.0.0.1:9527', secure: false });
    expect(proxy?.['/verify']).toMatchObject({ target: 'https://127.0.0.1:9527' });
    expect(proxy?.['/ws']).toMatchObject({ target: 'wss://127.0.0.1:9527', secure: false, ws: true });
  });
});
