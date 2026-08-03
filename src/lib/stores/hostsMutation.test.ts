import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./hosts.ts', import.meta.url), 'utf8');

function functionBody(name: string): string {
  const start = source.indexOf(`export async function ${name}`);
  expect(start).toBeGreaterThanOrEqual(0);
  const next = source.indexOf('\nexport async function ', start + 1);
  return source.slice(start, next < 0 ? source.length : next);
}

describe('remote host workspace mutation refresh contract', () => {
  it('refreshes linked topology after save succeeds', () => {
    const body = functionBody('saveHostWorkspace');
    const save = body.indexOf('await link.saveWorkspace(workspaceId, name)');
    const refresh = body.indexOf('await refreshLinkedHostAfterMutation(hostId)');
    expect(save).toBeGreaterThanOrEqual(0);
    expect(refresh).toBeGreaterThan(save);
  });
});
