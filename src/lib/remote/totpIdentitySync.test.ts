import { describe, it, expect, vi } from 'vitest';
import { writable } from 'svelte/store';
import { startTotpIdentitySync } from './totpIdentitySync';
import type { CloudAuthState } from './cloud/auth';

function state(username: string | null): CloudAuthState {
  return {
    userToken: username ? 'tok' : null,
    user: username ? ({ username } as unknown as CloudAuthState['user']) : null,
    deviceToken: null,
    deviceName: null,
  };
}

describe('startTotpIdentitySync', () => {
  it('登录时按 username 调命令，重复同值不再调，登出回 null', () => {
    const store = writable<CloudAuthState>(state(null));
    const invoke = vi.fn().mockResolvedValue(undefined);

    const stop = startTotpIdentitySync(invoke, store);
    // 初次订阅：当前 username=null。
    expect(invoke).toHaveBeenLastCalledWith('remote_set_totp_identity', { username: null });
    expect(invoke).toHaveBeenCalledTimes(1);

    store.set(state('alice'));
    expect(invoke).toHaveBeenLastCalledWith('remote_set_totp_identity', { username: 'alice' });
    expect(invoke).toHaveBeenCalledTimes(2);

    // 同一 username 再次推送（如 user 对象刷新）→ 不重复调用。
    store.set(state('alice'));
    expect(invoke).toHaveBeenCalledTimes(2);

    store.set(state(null));
    expect(invoke).toHaveBeenLastCalledWith('remote_set_totp_identity', { username: null });
    expect(invoke).toHaveBeenCalledTimes(3);

    stop();
  });
});
