import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

import { cloudHostOnline, refreshRemoteRunning, remoteRunning } from './remoteStatus';

beforeEach(() => {
	invokeMock.mockReset();
	remoteRunning.set(false);
	cloudHostOnline.set(false);
});

describe('remote runtime status', () => {
	it('reflects backend readiness instead of persisted settings', async () => {
		invokeMock.mockResolvedValue({ ready: true });
		expect(await refreshRemoteRunning()).toBe(true);
		expect(get(remoteRunning)).toBe(true);

		invokeMock.mockResolvedValue({ ready: false });
		expect(await refreshRemoteRunning()).toBe(false);
		expect(get(remoteRunning)).toBe(false);
	});

	it('fails closed when the backend cannot be queried', async () => {
		remoteRunning.set(true);
		invokeMock.mockRejectedValue(new Error('offline'));
		expect(await refreshRemoteRunning()).toBe(false);
		expect(get(remoteRunning)).toBe(false);
	});
});
