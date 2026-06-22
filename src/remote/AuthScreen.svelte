<script lang="ts">
  import { onMount } from 'svelte';
  import { t, tr } from '$lib/i18n';
  import { RemoteConnection, type ConnectionState } from './lib/wsRemote';
  import { getRemoteDeviceId } from './lib/deviceId';
  import CertTrustGuide from './CertTrustGuide.svelte';

  const TOKEN_KEY = 'ridge_remote_token';

  // §persist-login（任务 A 问题2）：登录态持久化。后端 device/user token 长期有效（user
  // 30 天 / device 180 天），所以 token 应「写一次、长期复用、刷新不丢」。localStorage 为
  // 主（跨刷新/跨标签页关闭仍在）；隐私模式或配额禁用 localStorage 时回退 sessionStorage
  // 兜底（至少当前会话内刷新不丢）。读取时两处都查，优先 localStorage。
  function readToken(): string | null {
    try {
      const v = localStorage.getItem(TOKEN_KEY);
      if (v) return v;
    } catch { /* localStorage blocked — fall through to sessionStorage */ }
    try { return sessionStorage.getItem(TOKEN_KEY); } catch { return null; }
  }
  function writeToken(token: string): void {
    let ok = false;
    try { localStorage.setItem(TOKEN_KEY, token); ok = true; } catch { /* blocked */ }
    // localStorage 不可用时才写 sessionStorage 兜底；可用则不重复写（避免两处不一致）。
    if (!ok) { try { sessionStorage.setItem(TOKEN_KEY, token); } catch { /* ignore */ } }
  }
  function clearToken(): void {
    try { localStorage.removeItem(TOKEN_KEY); } catch { /* ignore */ }
    try { sessionStorage.removeItem(TOKEN_KEY); } catch { /* ignore */ }
  }
  // Give up on a token (auto)connect after this long and fall back to the code
  // input — a stale/invalid token makes the server reject the /ws upgrade, and
  // wsRemote then RETRIES the drop forever ('disconnected', never 'error'), which
  // left the screen stuck on "connecting" indefinitely.
  const AUTH_CONNECT_TIMEOUT_MS = 9000;

  let { ws, onverified }: { ws: RemoteConnection; onverified: () => void } = $props();

  let code = $state('');
  let error = $state('');
  let loading = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();
  let showManual = $state(false);

  let unsubState: (() => void) | undefined;
  let connectTimer: ReturnType<typeof setTimeout> | undefined;

  function submitCode() {
    const numeric = code.replace(/\D/g, '').slice(0, 6);
    if (numeric.length < 6 || loading) return;
    code = '';
    loading = true;
    error = '';

    const host = location.hostname;
    const port = parseInt(location.port) || (location.protocol === 'https:' ? 443 : 80);

    fetch('/verify', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      // §L-3: bind the issued token to this device (in addition to its IP).
      body: `code=${encodeURIComponent(numeric)}&device=${encodeURIComponent(getRemoteDeviceId())}`,
    })
      .then(r => r.json())
      .then(d => {
        if (d.success && d.token) {
          writeToken(d.token);
          connectWithToken(host, port, d.token);
        } else {
          loading = false;
          error = d.message || tr('mobile.invalidCode');
        }
      })
      .catch(() => {
        loading = false;
        error = tr('mobile.networkError');
      });
  }

  // Abandon a failed (auto)connect: stop wsRemote's silent reconnect loop and fall
  // back to the manual code-entry screen.
  // §persist-login（任务 A 问题2）：只在确属「用户/鉴权类」失败（token 真的无效——
  // USERNAME_MISMATCH / DEVICE_* / NOT_PREMIUM / 4403 等，ws.lastFailure().category
  // === 'user'）时才清 token。纯通道/网络抖动导致的失败保留 token，这样刷新页面仍能
  // autoReconnect，不会因为一次网络波动就把一个其实长期有效的登录态丢掉。
  function fallbackToManual() {
    if (connectTimer) { clearTimeout(connectTimer); connectTimer = undefined; }
    unsubState?.();
    unsubState = undefined;
    const cat = ws.lastFailure()?.category;
    ws.disconnect(); // §stop-retry: otherwise it keeps retrying forever
    if (cat === 'user') clearToken(); // 凭据无效才丢弃；通道类保留以便刷新自动重连
    loading = false;
    showManual = true;
    error = tr('mobile.connectFail');
  }

  function connectWithToken(host: string, port: number, token: string) {
    unsubState?.();
    if (connectTimer) clearTimeout(connectTimer);
    let sawConnected = false;
    let disconnects = 0;
    // Safety net: a connect that never reaches 'connected' (hung upgrade / rejected
    // token) falls back to manual entry instead of an endless "connecting" spinner.
    connectTimer = setTimeout(fallbackToManual, AUTH_CONNECT_TIMEOUT_MS);
    unsubState = ws.onStateChange((s: ConnectionState) => {
      if (s === 'connected') {
        sawConnected = true;
        if (connectTimer) { clearTimeout(connectTimer); connectTimer = undefined; }
        loading = false;
        unsubState?.();
        onverified();
      } else if (s === 'error') {
        fallbackToManual();
      } else if (s === 'disconnected' && !sawConnected) {
        // The token auth keeps dropping before ever connecting (server rejected
        // the upgrade → wsRemote silently retries). Bail after a couple of tries
        // instead of spinning forever.
        if (++disconnects >= 2) fallbackToManual();
      }
    });
    ws.connect(host, port, token, 'token');
  }

  function autoReconnect() {
    const saved = readToken();
    if (!saved) {
      showManual = true;
      return;
    }
    loading = true;
    const host = location.hostname;
    const port = parseInt(location.port) || (location.protocol === 'https:' ? 443 : 80);
    connectWithToken(host, port, saved);
  }

  onMount(() => {
    autoReconnect();
    setTimeout(() => inputEl?.focus(), 400);
    return () => { unsubState?.(); if (connectTimer) clearTimeout(connectTimer); };
  });
</script>

{#if showManual}
  <div class="screen">
    <svg class="logo" viewBox="0 0 32 32" fill="none" aria-label="Ridge mark">
      <rect x="2.5" y="2.5" width="27" height="27" rx="6" stroke="#7fb069" stroke-width="2"/>
      <line x1="16" y1="3.5" x2="16" y2="28.5" stroke="#7fb069" stroke-width="2"/>
      <line x1="3.5" y1="16" x2="28.5" y2="16" stroke="#7fb069" stroke-width="2"/>
      <rect x="4.5" y="4.5" width="9.5" height="9.5" rx="2" fill="#7fb069" fill-opacity="0.18"/>
      <rect x="18" y="18" width="9.5" height="9.5" rx="2" fill="#d97757" fill-opacity="0.22"/>
    </svg>
    <h1>Ridge Remote</h1>
    <p class="sub">{$t('mobile.authSubtitle')}</p>
    <div class="card">
      <input
        bind:this={inputEl}
        type="text" inputmode="numeric" maxlength={6}
        placeholder={$t('mobile.codePlaceholder')}
        oninput={(e) => { code = (e.target as HTMLInputElement).value.replace(/\D/g, '').slice(0, 6); error = ''; }}
        onkeydown={(e) => { if (e.key === 'Enter') submitCode(); }}
        class:has-error={!!error}
      />
      {#if error}<p class="error-msg">{error}</p>{/if}
      <button onclick={submitCode} disabled={code.length < 6 || loading}>
        {loading ? $t('mobile.verifying') : $t('mobile.verifyAndConnect')}
      </button>
    </div>
    <CertTrustGuide />
  </div>
{:else if loading}
  <div class="screen">
    <svg class="logo" viewBox="0 0 32 32" fill="none" aria-label="Ridge mark">
      <rect x="2.5" y="2.5" width="27" height="27" rx="6" stroke="#7fb069" stroke-width="2"/>
      <line x1="16" y1="3.5" x2="16" y2="28.5" stroke="#7fb069" stroke-width="2"/>
      <line x1="3.5" y1="16" x2="28.5" y2="16" stroke="#7fb069" stroke-width="2"/>
      <rect x="4.5" y="4.5" width="9.5" height="9.5" rx="2" fill="#7fb069" fill-opacity="0.18"/>
      <rect x="18" y="18" width="9.5" height="9.5" rx="2" fill="#d97757" fill-opacity="0.22"/>
    </svg>
    <p class="sub">{$t('mobile.connecting')}</p>
  </div>
{/if}

<style>
  .screen{position:fixed;inset:0;background:var(--rg-bg);display:flex;flex-direction:column;align-items:center;justify-content:center;padding:24px}
  .logo{display:block;width:64px;height:64px;margin:0 auto 16px}
  h1{font-size:20px;font-weight:600;margin-bottom:4px;color:var(--rg-fg)}
  .sub{color:var(--rg-fg-muted);font-size:14px;margin-bottom:24px;text-align:center;line-height:1.5}
  .card{width:100%;max-width:340px;background:var(--rg-surface);border:1px solid var(--rg-border-bright);border-radius:12px;padding:24px;text-align:center}
  input{width:100%;height:48px;padding:0 16px;border:2px solid var(--rg-border-bright);border-radius:10px;background:var(--rg-bg);color:var(--rg-fg);font-size:24px;font-weight:700;letter-spacing:8px;text-align:center;outline:none;transition:border-color .2s}
  input:focus{border-color:var(--rg-accent)}
  input.has-error{border-color:var(--rg-ansi-red)}
  input::placeholder{color:var(--rg-fg-muted);letter-spacing:2px;font-size:14px}
  .error-msg{color:var(--rg-ansi-red);font-size:13px;margin-top:8px}
  button{width:100%;height:48px;border:none;border-radius:10px;font-size:16px;font-weight:600;cursor:pointer;transition:opacity .2s;margin-top:16px;background:var(--rg-ansi-green);color:#fff}
  button:disabled{opacity:.4;cursor:not-allowed}
  button:hover:not(:disabled){background:var(--rg-ansi-green)}
</style>
