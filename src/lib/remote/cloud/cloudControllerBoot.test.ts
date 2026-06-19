// src/lib/remote/cloud/cloudControllerBoot.test.ts
//
// Unit tests for the cloud-controller trigger parsing. The tenant-hostname
// grammar is security-relevant: it decides whether a browser auto-enters
// cloud-controller mode for a given host, so it MUST stay byte-for-byte aligned
// with the ridge-cloud backend `validation.rs` §1.1/§1.2 (`parse_tenant_label`).
// Covers:
//   • parseCloudControllerHostname: valid tenant split on the LAST dash
//   • reserved subdomain labels are never tenants
//   • IP / dot-less / dash-less hosts are not tenants (LAN fallback)
//   • device/username length + charset + double-dash rejection
//   • case-insensitive normalization + port stripping
//   • parseCloudControllerUrl: query precedence shape

import { describe, it, expect, vi } from 'vitest';

// `auth.ts` reads localStorage at module load; the vitest `node` env has no real
// localStorage, so stub the one symbol cloudControllerBoot imports. The parsers
// under test never touch auth — this only keeps the module graph importable.
vi.mock('./auth', () => ({
  snapshot: () => ({ userToken: null, user: null, deviceToken: null, deviceName: null }),
}));

import {
  parseCloudControllerHostname,
  parseCloudControllerUrl,
  verifyTotpOverControl,
  performTrustHandshake,
} from './cloudControllerBoot';
import type { CloudWebrtcAdapter } from '$lib/transport/remote/cloudWebrtcAdapter';
import { buildBindTranscript, computeBindTag, bytesToBase64 } from './e2ee';

/**
 * Minimal fake of the bits of CloudWebrtcAdapter that verifyTotpOverControl uses
 * (§4 CONTROL channel): captures the outbound `totp-verify` and lets the test
 * drive the inbound `totp-result`.
 */
function makeFakeAdapter() {
  let listener: ((frame: Record<string, unknown>) => void) | null = null;
  const sent: Record<string, unknown>[] = [];
  const adapter = {
    sendSessionControl: (frame: Record<string, unknown>) => sent.push(frame),
    onSessionControl: (cb: (frame: Record<string, unknown>) => void) => {
      listener = cb;
      return () => {
        if (listener === cb) listener = null;
      };
    },
  } as unknown as CloudWebrtcAdapter;
  return {
    adapter,
    sent,
    emit: (frame: Record<string, unknown>) => listener?.(frame),
    hasListener: () => listener !== null,
  };
}

describe('parseCloudControllerHostname', () => {
  it('splits a tenant host on the LAST dash into device + username', () => {
    // Arrange
    const host = 'my-laptop-alice.9527127.xyz';
    // Act
    const parsed = parseCloudControllerHostname(host);
    // Assert
    expect(parsed).toEqual({ hostDevice: 'my-laptop', username: 'alice' });
  });

  it('accepts a single-segment device name', () => {
    expect(parseCloudControllerHostname('host-bob.example.com')).toEqual({
      hostDevice: 'host',
      username: 'bob',
    });
  });

  it('normalizes case and strips a port before parsing', () => {
    expect(parseCloudControllerHostname('My-Laptop-Alice.example.com:443')).toEqual({
      hostDevice: 'my-laptop',
      username: 'alice',
    });
  });

  it.each(['www', 'api', 'ws', 'app', 'admin', 'static', 'cdn', 'mail'])(
    'treats reserved label %s as a system host, not a tenant',
    (label) => {
      expect(parseCloudControllerHostname(`${label}.9527127.xyz`)).toBeNull();
    },
  );

  it('returns null for dash-less / dot-less / localhost hosts', () => {
    expect(parseCloudControllerHostname('localhost')).toBeNull();
    expect(parseCloudControllerHostname('9527127.xyz')).toBeNull();
    expect(parseCloudControllerHostname('mydevice.example.com')).toBeNull();
  });

  it('returns null for IPv4 literals (LAN access falls back)', () => {
    expect(parseCloudControllerHostname('192.168.1.5')).toBeNull();
    expect(parseCloudControllerHostname('127.0.0.1')).toBeNull();
  });

  it('rejects a username shorter than 3 or longer than 20', () => {
    expect(parseCloudControllerHostname('dev-ab.example.com')).toBeNull(); // 2 chars
    const long = 'u'.repeat(21);
    expect(parseCloudControllerHostname(`dev-${long}.example.com`)).toBeNull();
  });

  it('rejects a device shorter than 3 characters', () => {
    expect(parseCloudControllerHostname('ab-alice.example.com')).toBeNull();
  });

  it('rejects a device containing a double dash', () => {
    // last dash splits → device "my--host" (contains "--") → invalid.
    expect(parseCloudControllerHostname('my--host-alice.example.com')).toBeNull();
  });

  it('rejects a username with non-[a-z0-9] characters', () => {
    expect(parseCloudControllerHostname('host-al_ce.example.com')).toBeNull();
    expect(parseCloudControllerHostname('host-al.ce.example.com')).toBeNull(); // dot ends the label early
  });

  it('rejects a trailing dash (empty username)', () => {
    expect(parseCloudControllerHostname('host-.example.com')).toBeNull();
  });

  it('returns null for an empty hostname', () => {
    expect(parseCloudControllerHostname('')).toBeNull();
  });
});

describe('parseCloudControllerUrl', () => {
  it('extracts hostDevice and username from query params', () => {
    expect(parseCloudControllerUrl('?cloudHost=my-laptop&u=alice')).toEqual({
      hostDevice: 'my-laptop',
      username: 'alice',
    });
  });

  it('leaves username undefined when only cloudHost is given (boot fills from cloudAuth)', () => {
    expect(parseCloudControllerUrl('?cloudHost=my-laptop')).toEqual({
      hostDevice: 'my-laptop',
      username: undefined,
    });
  });

  it('returns null when cloudHost is absent', () => {
    expect(parseCloudControllerUrl('')).toBeNull();
    expect(parseCloudControllerUrl('?u=alice')).toBeNull();
  });
});

describe('verifyTotpOverControl (§4 controller→host TOTP handshake)', () => {
  it('sends totp-verify on the CONTROL channel and resolves true on ok result', async () => {
    const fake = makeFakeAdapter();
    const p = verifyTotpOverControl(fake.adapter, '123456');
    expect(fake.sent).toEqual([{ t: 'totp-verify', code: '123456' }]);

    fake.emit({ t: 'totp-result', ok: true });
    await expect(p).resolves.toBe(true);
    // Listener was cleaned up after settling.
    expect(fake.hasListener()).toBe(false);
  });

  it('sends totp-bind (HMAC tag, no plaintext code) when a bind transcript is available', async () => {
    const fake = makeFakeAdapter();
    // 模拟已收到 host 0x02：adapter 暴露信道绑定 transcript（零信任 #1）。
    const transcript = buildBindTranscript(
      new Uint8Array(32).fill(0x11),
      new Uint8Array(32).fill(0x22),
    );
    (fake.adapter as unknown as { getBindTranscript: () => Uint8Array }).getBindTranscript = () =>
      transcript;
    const p = verifyTotpOverControl(fake.adapter, '123456');
    // 发出的是 totp-bind（base64 HMAC tag）而非明文 totp-verify；明文码不上线。
    expect(fake.sent.length).toBe(1);
    const frame = fake.sent[0];
    expect(frame.t).toBe('totp-bind');
    expect(frame.code).toBeUndefined();
    expect(frame.tag).toBe(bytesToBase64(computeBindTag('123456', transcript)));
    fake.emit({ t: 'totp-result', ok: true });
    await expect(p).resolves.toBe(true);
  });

  it('resolves false on a totp-result{ok:false}', async () => {
    const fake = makeFakeAdapter();
    const p = verifyTotpOverControl(fake.adapter, '000000');
    fake.emit({ t: 'totp-result', ok: false });
    await expect(p).resolves.toBe(false);
  });

  it('ignores non-result CONTROL frames until the real result arrives', async () => {
    const fake = makeFakeAdapter();
    const p = verifyTotpOverControl(fake.adapter, '123456');
    fake.emit({ t: 'some-other-control' }); // not a totp-result
    fake.emit({ t: 'totp-result', ok: true });
    await expect(p).resolves.toBe(true);
  });

  it('rejects on timeout when no result arrives, and unsubscribes', async () => {
    vi.useFakeTimers();
    try {
      const fake = makeFakeAdapter();
      const p = verifyTotpOverControl(fake.adapter, '123456', 5000);
      const assertion = expect(p).rejects.toThrow();
      await vi.advanceTimersByTimeAsync(5000);
      await assertion;
      expect(fake.hasListener()).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('performTrustHandshake (§7.4 trusted-controller grant)', () => {
  it('sends totp-trust-hello and resolves false on timeout when no challenge arrives (old host), then unsubscribes', async () => {
    // 旧 host 不识别 totp-trust-hello → 永不回 totp-trust-challenge。控制端必须在
    // 超时后 resolve(false)（退化到 TOTP），绝不 reject / 悬挂。用短超时跑真实计时器，
    // 避免与 getControllerPub() 的 IndexedDB-降级异步路径在假计时器下相互干扰。
    const fake = makeFakeAdapter();
    const trusted = await performTrustHandshake(fake.adapter, 20);
    expect(trusted).toBe(false);
    // 已发出 hello（公钥 32B base64），且超时后退订（无悬挂监听）。
    expect(fake.sent.length).toBe(1);
    expect(fake.sent[0].t).toBe('totp-trust-hello');
    expect(typeof fake.sent[0].pub).toBe('string');
    expect(fake.hasListener()).toBe(false);
  });
});
