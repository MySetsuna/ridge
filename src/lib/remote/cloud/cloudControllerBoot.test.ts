import { describe, expect, it } from 'vitest';
import {
  activeCloudController,
  bootCloudControllerFromUrl,
  parseCloudControllerHostname,
  parseCloudControllerUrl,
} from './cloudControllerBoot';

describe('cloud controller tenant URL parsing', () => {
  it('prefers explicit query parameters and preserves encoded values', () => {
    expect(parseCloudControllerUrl('?cloudHost=my-laptop&u=alice')).toEqual({
      hostDevice: 'my-laptop', username: 'alice',
    });
    expect(parseCloudControllerUrl('?cloudHost=my-laptop')).toEqual({ hostDevice: 'my-laptop' });
    expect(parseCloudControllerUrl('')).toBeNull();
  });

  it('validates tenant hostname labels and rejects reserved or malformed hosts', () => {
    expect(parseCloudControllerHostname('my-laptop-alice.9527127.xyz')).toEqual({
      hostDevice: 'my-laptop', username: 'alice',
    });
    expect(parseCloudControllerHostname('MY-LAPTOP-ALICE:443')).toEqual({
      hostDevice: 'my-laptop', username: 'alice',
    });
    for (const hostname of [
      'www.example.com', 'missing-x.example.com', 'ab-alice.example.com',
      'laptop-ab.example.com', 'laptop-alice--x.example.com', 'laptop-a--b.example.com',
    ]) {
      expect(parseCloudControllerHostname(hostname)).toBeNull();
    }
  });

  it('returns null outside cloud-controller mode or when boot credentials are absent', () => {
    expect(activeCloudController()).toBeNull();
    expect(bootCloudControllerFromUrl('', undefined, 'app.example.com')).toBeNull();
    expect(bootCloudControllerFromUrl('?cloudHost=laptop&u=alice')).toBeNull();
    expect(bootCloudControllerFromUrl('', undefined, 'laptop-alice.example.com')).toBeNull();
  });
});
