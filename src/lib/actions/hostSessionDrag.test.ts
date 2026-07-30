import { describe, expect, it } from 'vitest';
import { hostAttachRequestAt } from './hostSessionDrag';

describe('hostSessionDrag attach routing', () => {
  it('keeps remote identity and dock target on the shared attach request', () => {
    expect(hostAttachRequestAt({
      kind: 'remote',
      socket: 'cloud:phone',
      name: 'shell',
      hostId: 'cloud:phone',
      sessionId: 'remote-pane',
      workspaceId: 'remote-workspace',
    }, 'local-target', 'right')).toEqual({
      kind: 'remote',
      socket: 'cloud:phone',
      target: 'shell',
      hostId: 'cloud:phone',
      sessionId: 'remote-pane',
      workspaceId: 'remote-workspace',
      targetPaneId: 'local-target',
      region: 'right',
    });
  });

  it('keeps native drag on the same request shape', () => {
    expect(hostAttachRequestAt({
      kind: 'headless',
      socket: 'headless',
      name: 'build',
    }, 'target', 'left')).toMatchObject({
      kind: 'headless',
      socket: 'headless',
      target: 'build',
      targetPaneId: 'target',
      region: 'left',
    });
  });
});
