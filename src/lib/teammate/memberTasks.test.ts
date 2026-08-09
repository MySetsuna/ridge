import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

import { memberTasksStore, recordMemberTask } from './memberTasks';

afterEach(() => vi.unstubAllGlobals());

describe('member task persistence', () => {
	it('ignores empty identity or task and persists valid latest work', () => {
		const data = new Map<string, string>();
		const storage = {
			getItem: vi.fn((key: string) => data.get(key) ?? null),
			setItem: vi.fn((key: string, value: string) => data.set(key, value)),
		};
		vi.stubGlobal('localStorage', storage);
		const before = get(memberTasksStore);
		recordMemberTask('', 'ignored');
		recordMemberTask('agent-test', '');
		expect(get(memberTasksStore)).toEqual(before);

		recordMemberTask('agent-test', 'ship it');
		expect(get(memberTasksStore)['agent-test']).toMatchObject({ text: 'ship it' });
		expect(storage.setItem).toHaveBeenCalledWith('ridge.memberTasks.v1', expect.any(String));
	});
});
