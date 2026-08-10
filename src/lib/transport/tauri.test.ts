import { describe, expect, it, vi } from 'vitest';
import { TauriDataProvider, type DataInvoke } from './tauri';

describe('TauriDataProvider injected invocation', () => {
  it('routes filesystem calls through the isolated invoker', async () => {
    const call = vi.fn(async (method: string) => {
      if (method === 'get_file_tree') return { path: '/shared', children: [] };
      if (method === 'read_file') return 'shared-content';
      throw new Error(`unexpected ${method}`);
    }) as DataInvoke;
    const provider = new TauriDataProvider(call);

    await expect(provider.getFileTree('/shared', 1)).resolves.toMatchObject({ path: '/shared' });
    await expect(provider.readFile('/shared/a.txt')).resolves.toBe('shared-content');
    expect(call).toHaveBeenNthCalledWith(1, 'get_file_tree', { path: '/shared', depth: 1 });
    expect(call).toHaveBeenNthCalledWith(2, 'read_file', { path: '/shared/a.txt' });
	});

	it('routes filesystem mutations and optional directory limits', async () => {
		const call = vi.fn(async () => undefined);
		const provider = new TauriDataProvider(call as unknown as DataInvoke);

		await provider.getDirectoryChildren('/repo', 4);
		await provider.getDirectoryChildren('/repo', 4, 20);
		await provider.pathExists('/repo/a');
		await provider.writeFile('/repo/a', 'content');
		await provider.renamePath('/repo/a', '/repo/b');
		await provider.deletePath('/repo/b');
		await provider.createFile('/repo/c');
		await provider.createDirectory('/repo/dir');
		await provider.copyPath('/repo/c', '/repo/d');
		await provider.movePath('/repo/d', '/repo/e');
		await provider.revealInFileManager('/repo');

		expect(call).toHaveBeenNthCalledWith(1, 'get_directory_children', { path: '/repo', offset: 4 });
		expect(call).toHaveBeenNthCalledWith(2, 'get_directory_children', { path: '/repo', offset: 4, limit: 20 });
		expect(call).toHaveBeenLastCalledWith('reveal_in_file_manager', { path: '/repo' });
	});

	it('maps SCM, graph, and search backend shapes to shared contracts', async () => {
		const call = vi.fn(async (method: string) => {
			if (method === 'get_scm_status') {
				return {
					is_git_repo: true,
					current_branch: 'main',
					has_upstream: true,
					staged: [{ path: 'staged.ts', status: 'A' }],
					changes: [{ path: 'changed.ts', status: 'M' }],
					untracked: [{ path: 'new.ts', status: '?' }],
				};
			}
			if (method === 'get_git_info_with_cwd') {
				return {
					current_branch: 'main',
					branches: ['main', 'feature'],
					commits: [{ hash: 'abc', subject: 'subject', date: 'today', parents: ['root'], refs: ['HEAD'] }],
				};
			}
			if (method === 'git_list_branches') return [{ name: 'main' }, {}, { name: 'feature' }];
			if (method === 'get_git_commits_paginated') return [{ hash: 'def', subject: 'graph', date: 'now' }];
			if (method === 'get_current_project') return '/active';
			if (method === 'text_search') return [{ file: 'src/a.ts', line: 3, column: 5, content: 'hit' }];
			throw new Error(`unexpected ${method}`);
		});
		const provider = new TauriDataProvider(call as unknown as DataInvoke);

		await expect(provider.gitStatus('/repo')).resolves.toEqual({
			is_git_repo: true,
			current_branch: 'main',
			has_upstream: true,
			branches: ['main', 'feature'],
			staged: [{ name: 'staged.ts', status: 'A' }],
			unstaged: [{ name: 'changed.ts', status: 'M' }],
			untracked: ['new.ts'],
			commits: [{ hash: 'abc', msg: 'subject', time: 'today', parents: ['root'], refs: ['HEAD'] }],
		});
		await expect(provider.gitGraph('/repo')).resolves.toEqual({
			branches: ['main', 'feature'],
			commits: [{ hash: 'def', msg: 'graph', time: 'now' }],
		});
		await expect(provider.searchFiles('needle')).resolves.toEqual([
			{ path: 'src/a.ts', line: 3, column: 5, snippet: 'hit' },
		]);
		await expect(provider.searchFiles('  ')).resolves.toEqual([]);
		await expect(provider.searchFiles('needle', ' /repo ')).resolves.toEqual([
			{ path: 'src/a.ts', line: 3, column: 5, snippet: 'hit' },
		]);
		expect(call).toHaveBeenCalledWith('text_search', { root: '/repo', query: 'needle', maxResults: 500 });
	});

	it('forwards every git mutation with stable defaults', async () => {
		const call = vi.fn(async () => undefined);
		const provider = new TauriDataProvider(call as unknown as DataInvoke);
		await provider.gitStage('/r', ['a']);
		await provider.gitUnstage('/r', ['a']);
		await provider.gitCommit('/r', 'msg');
		await provider.gitCommit('/r', 'amend', true);
		await provider.gitPull('/r');
		await provider.gitPush('/r');
		await provider.gitPush('/r', true);
		await provider.gitSync('/r');
		await provider.gitCheckout('/r', 'main');
		await provider.gitCheckout('/r', 'new', true);
		await provider.gitRevert('/r', 'abc');
		await provider.gitCherryPick('/r', 'def');
		await provider.gitReset('/r', 'mixed', 'ghi');
		await provider.gitCreateTag('/r', 'v1');
		await provider.gitCreateTag('/r', 'v2', 'release');
		await provider.gitDiscard('/r', ['a']);
		await provider.gitCleanUntracked('/r');
		await provider.gitDiffFile('/r', 'a');

		expect(call).toHaveBeenCalledWith('git_commit', { repoRoot: '/r', message: 'msg', amend: false });
		expect(call).toHaveBeenCalledWith('git_push', { repoRoot: '/r', setUpstream: false });
		expect(call).toHaveBeenCalledWith('git_checkout', { repoRoot: '/r', branch: 'main', create: false });
		expect(call).toHaveBeenCalledWith('git_create_tag', { repoRoot: '/r', name: 'v1', message: '' });
		expect(call).toHaveBeenCalledWith('git_diff_file', { repoRoot: '/r', path: 'a', cached: false });
	});
});
