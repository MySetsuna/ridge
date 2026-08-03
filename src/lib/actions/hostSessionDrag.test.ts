import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { paneDockHover, paneDragSourceId } from '$lib/stores/paneTree';
import { HOST_SESSION_DRAG_SOURCE, hostAttachRequestAt, hostSessionDrag } from './hostSessionDrag';

function eventOf(type: string, init: Record<string, unknown> = {}): Event {
  const event = new Event(type);
  for (const [key, value] of Object.entries(init)) {
    Object.defineProperty(event, key, { configurable: true, value });
  }
  return event;
}

describe('hostSessionDrag attach routing', () => {
  afterEach(() => {
    paneDragSourceId.set(null);
    paneDockHover.set(null);
    vi.unstubAllGlobals();
  });

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

  it('cleans the drag sentinel on pointer cancellation and window blur', () => {
    const node = new EventTarget() as EventTarget & { closest: () => null };
    node.closest = () => null;
    const win = new EventTarget();
    const body = { style: { cursor: '' } };
    vi.stubGlobal('window', win);
    vi.stubGlobal('document', { body, elementFromPoint: () => null });

    const drag = hostSessionDrag(node as unknown as HTMLElement, {
      kind: 'remote',
      socket: 'lan:host',
      name: 'shell',
      hostId: 'lan:host',
      sessionId: 'pane-1',
    });

    node.dispatchEvent(eventOf('pointerdown', {
      button: 0,
      clientX: 10,
      clientY: 10,
      target: node,
    }));
    win.dispatchEvent(eventOf('pointermove', { clientX: 30, clientY: 30 }));
    expect(get(paneDragSourceId)).toBe(HOST_SESSION_DRAG_SOURCE);
    expect(body.style.cursor).toBe('grabbing');

    win.dispatchEvent(eventOf('pointercancel'));
    expect(get(paneDragSourceId)).toBeNull();
    expect(get(paneDockHover)).toBeNull();
    expect(body.style.cursor).toBe('');

    // Blur is an independent cancellation path and must stay idempotent.
    win.dispatchEvent(eventOf('blur'));
    expect(get(paneDragSourceId)).toBeNull();
    drag.destroy();
  });
});
