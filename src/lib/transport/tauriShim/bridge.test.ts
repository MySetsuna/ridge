import { describe, expect, it } from 'vitest';
import type {
  AuthListener,
  ChannelTransport,
  ControlListener,
  OutboundFrame,
  PaneBytesListener,
  StateListener,
} from '@ridge/remote';
import { TauriBridge } from './bridge';

function rig() {
  const sent: OutboundFrame[] = [];
  let disposed = 0;
  const subscribe = <T>(_cb: T) => () => { disposed += 1; };
  const transport: ChannelTransport = {
    sendControl: (frame) => sent.push(frame),
    onControl: (cb: ControlListener) => subscribe(cb),
    sendPaneBytes: () => {},
    onPaneBytes: (cb: PaneBytesListener) => subscribe(cb),
    connect: () => {},
    close: () => {},
    state: () => 'connected',
    onStateChange: (cb: StateListener) => subscribe(cb),
    authState: () => 'authorized',
    onAuthChange: (cb: AuthListener) => subscribe(cb),
  };
  return { transport, sent, disposed: () => disposed };
}

describe('TauriBridge isolated attachment', () => {
  it('negotiates hello without enabling global workspace semantics', () => {
    const { transport, sent, disposed } = rig();
    const bridge = new TauriBridge();
    bridge.attach(transport, { useGlobalWorkspace: false });

    expect(bridge.ready).toBe(true);
    expect(sent.some((frame) => 'method' in frame && frame.method === '$/hello')).toBe(true);
    expect(sent.some((frame) => 'method' in frame && frame.method === 'use-global-workspace')).toBe(false);

    bridge.detach();
    expect(bridge.ready).toBe(false);
    expect(disposed()).toBeGreaterThan(0);
  });
});
