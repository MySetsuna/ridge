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
});
