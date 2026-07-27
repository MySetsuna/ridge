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

  it.each([
    ['9527127.xyz', '', 'cloud'],
    ['device-alice.9527127.xyz', '', 'cloud'],
    ['DEVICE-ALICE.9527127.XYZ.', '', 'cloud'],
    ['172.21.130.235', '?cloudHost=rdg-1&u=alice', 'cloud'],
  ])('keeps %s on the cloud boot path', (hostname, search, expected) => {
    expect(remoteBootMode(hostname, search, '9527127.xyz:443')).toBe(expected);
  });
});
