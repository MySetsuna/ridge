import { describe, expect, it } from 'vitest';
import { remoteBootMode } from './remoteBootMode';

describe('remoteBootMode', () => {
  it.each([
    ['172.21.130.235', '', 'lan'],
    ['localhost', '', 'lan'],
    ['ridge-box.local', '', 'lan'],
    ['ridge.internal.example', '', 'lan'],
  ])('boots %s directly through LAN', (hostname, search, expected) => {
    expect(remoteBootMode(hostname, search, '9527127.xyz')).toBe(expected);
  });

  it('keeps a local dev server on LAN when cloud uses another port', () => {
    expect(remoteBootMode('localhost:5174', '', 'localhost:5001')).toBe('lan');
  });

  it('uses cloud when the local cloud port is the requested port', () => {
    expect(remoteBootMode('localhost:5001', '', 'localhost:5001')).toBe('cloud');
  });

  it.each([
    ['9527127.xyz', '', 'cloud'],
    ['device-alice.9527127.xyz', '', 'cloud'],
    ['DEVICE-ALICE.9527127.XYZ.', '', 'cloud'],
    ['172.21.130.235', '?cloudHost=rdg-1&u=alice', 'cloud'],
  ])('keeps %s on the cloud boot path', (hostname, search, expected) => {
    expect(remoteBootMode(hostname, search, '9527127.xyz:443')).toBe(expected);
  });

  it('matches public IPv6 hosts without confusing colons for a port', () => {
    expect(remoteBootMode('[2001:db8::10]:5174', '', '[2001:db8::10]:443')).toBe('cloud');
    expect(remoteBootMode('2001:db8::10', '', '2001:db8::10')).toBe('cloud');
  });
});
