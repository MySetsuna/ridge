import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	activeCloudController,
	bootCloudControllerFromUrl,
	performTrustHandshake,
	parseCloudControllerHostname,
	parseCloudControllerUrl,
	verifyTotpOverControl,
} from './cloudControllerBoot';
import { bytesToBase64 } from '@ridge/remote/shared/cloud/e2ee';

afterEach(() => vi.useRealTimers());

describe('cloud controller tenant URL parsing', () => {
  it('prefers explicit query parameters and preserves encoded values', () => {
    expect(parseCloudControllerUrl('?cloudHost=my-laptop&u=alice')).toEqual({
      hostDevice: 'my-laptop', username: 'alice',
    });
    expect(parseCloudControllerUrl('?cloudHost=my-laptop')).toEqual({ hostDevice: 'my-laptop' });
    expect(parseCloudControllerUrl('')).toBeNull();
  });

  it('validates tenant hostname labels and rejects reserved or malformed hosts', () => {
    expect(parseCloudControllerHostname('my-laptop-alice.9527127.xyz')).toEqual({
      hostDevice: 'my-laptop', username: 'alice',
    });
    expect(parseCloudControllerHostname('MY-LAPTOP-ALICE:443')).toEqual({
      hostDevice: 'my-laptop', username: 'alice',
    });
    for (const hostname of [
      'www.example.com', 'missing-x.example.com', 'ab-alice.example.com',
      'laptop-ab.example.com', 'laptop-alice--x.example.com', 'laptop-a--b.example.com',
    ]) {
      expect(parseCloudControllerHostname(hostname)).toBeNull();
    }
  });

	it('returns null outside cloud-controller mode or when boot credentials are absent', () => {
    expect(activeCloudController()).toBeNull();
    expect(bootCloudControllerFromUrl('', undefined, 'app.example.com')).toBeNull();
    expect(bootCloudControllerFromUrl('?cloudHost=laptop&u=alice')).toBeNull();
		expect(bootCloudControllerFromUrl('', undefined, 'laptop-alice.example.com')).toBeNull();
	});

	it('binds TOTP to the transcript and resolves the host result', async () => {
		let receive: ((frame: { t: string; ok?: boolean }) => void) | undefined;
		const adapter = {
			onSessionControl: (cb: typeof receive) => { receive = cb; return vi.fn(); },
			sendSessionControl: vi.fn(),
			getBindTranscript: () => Uint8Array.from([1, 2, 3]),
		};
		const pending = verifyTotpOverControl(adapter as never, '123456', 1000);
		expect(adapter.sendSessionControl).toHaveBeenCalledWith(expect.objectContaining({ t: 'totp-bind' }));
		receive?.({ t: 'totp-result', ok: true });
		expect(await pending).toBe(true);
	});

	it('falls back to plaintext TOTP and rejects on timeout', async () => {
		vi.useFakeTimers();
		let receive: ((frame: { t: string; ok?: boolean }) => void) | undefined;
		const adapter = {
			onSessionControl: (cb: typeof receive) => { receive = cb; return vi.fn(); },
			sendSessionControl: vi.fn(),
		};
		const pending = verifyTotpOverControl(adapter as never, '654321', 20);
		const rejected = expect(pending).rejects.toThrow();
		expect(adapter.sendSessionControl).toHaveBeenCalledWith({ t: 'totp-verify', code: '654321' });
		await vi.advanceTimersByTimeAsync(20);
		await rejected;
		vi.useRealTimers();
	});

	it('completes the trust grant handshake only after a valid challenge and result', async () => {
		let receive: ((frame: { t: string; nonce?: string; trusted?: boolean }) => void) | undefined;
		const adapter = {
			onSessionControl: (cb: typeof receive) => { receive = cb; return vi.fn(); },
			sendSessionControl: vi.fn(),
			getBindTranscript: () => Uint8Array.from([4, 5, 6]),
		};
		const pending = performTrustHandshake(adapter as never, 1000);
		await new Promise<void>((resolve) => setTimeout(resolve, 0));
		expect(adapter.sendSessionControl).toHaveBeenCalledWith(expect.objectContaining({ t: 'totp-trust-hello' }));
		receive?.({ t: 'totp-trust-challenge', nonce: bytesToBase64(new Uint8Array(32).fill(7)) });
		await Promise.resolve();
		await Promise.resolve();
		expect(adapter.sendSessionControl).toHaveBeenCalledWith(expect.objectContaining({ t: 'totp-trust-proof' }));
		receive?.({ t: 'totp-trust-result', trusted: true });
		expect(await pending).toBe(true);
	});

	it('fails closed for malformed trust challenges and timeout', async () => {
		let receive: ((frame: { t: string; nonce?: string; trusted?: boolean }) => void) | undefined;
		const adapter = {
			onSessionControl: (cb: typeof receive) => { receive = cb; return vi.fn(); },
			sendSessionControl: vi.fn(),
		};
		const malformed = performTrustHandshake(adapter as never, 1000);
		await new Promise<void>((resolve) => setTimeout(resolve, 0));
		receive?.({ t: 'totp-trust-challenge', nonce: 'bad' });
		expect(await malformed).toBe(false);

		vi.useFakeTimers();
		const timedOut = performTrustHandshake(adapter as never, 25);
		await Promise.resolve();
		await Promise.resolve();
		await vi.advanceTimersByTimeAsync(25);
		expect(await timedOut).toBe(false);
	});
});
