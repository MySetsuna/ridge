import { describe, expect, it } from 'vitest';
import { assertShareTokenScope } from './sharedWorkspaceProjection';

const input = {
  grantId: 'grant-1',
  workspaceId: 'workspace-1',
  deviceName: 'host-1',
};

describe('assertShareTokenScope', () => {
  it('accepts the exact non-delegable scope', () => {
    expect(() => assertShareTokenScope(input, {
      ...input,
      delegable: false,
    })).not.toThrow();
  });

  it.each([
    [{ ...input, grantId: 'grant-2', delegable: false }],
    [{ ...input, workspaceId: 'workspace-2', delegable: false }],
    [{ ...input, deviceName: 'host-2', delegable: false }],
    [{ ...input, delegable: true }],
  ])('rejects mismatched or delegable scope', (scoped) => {
    expect(() => assertShareTokenScope(input, scoped)).toThrow('授权范围不匹配');
  });
});
