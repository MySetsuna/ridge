import { describe, expect, it } from 'vitest';
import { remoteWebSocketUrl } from './wsRemote';

describe('remoteWebSocketUrl', () => {
  it('lets desktop LAN explicitly select wss independent of page scheme', () => {
    expect(remoteWebSocketUrl({
      host: '192.168.1.5',
      port: 9528,
      auth: '123 456',
      authType: 'code',
      device: 'desktop/a',
      secure: true,
    })).toBe('wss://192.168.1.5:9528/ws?code=123%20456&device=desktop%2Fa');
  });

  it('keeps explicit plaintext LAN endpoints on ws', () => {
    expect(remoteWebSocketUrl({
      host: '127.0.0.1',
      port: 7522,
      auth: 'token',
      authType: 'token',
      device: 'dev',
      secure: false,
    })).toBe('ws://127.0.0.1:7522/ws?token=token&device=dev');
  });
});
