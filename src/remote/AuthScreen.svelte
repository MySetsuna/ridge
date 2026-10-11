<script lang="ts">
  import { onMount } from 'svelte';
  import { t, tr } from '$lib/i18n';
  import { RemoteConnection, type ConnectionState } from '@ridge/remote';
  import { getRemoteDeviceId } from '@ridge/remote';
  import CertTrustGuide from './CertTrustGuide.svelte';
  import { readRemoteToken, writeRemoteToken, clearRemoteToken, remoteSessionValidity } from './lib/remoteSession';

  let { ws, onverified }: { ws: RemoteConnection; onverified: () => void } = $props();

  let code = $state('');
  let error = $state('');
  let loading = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();
  let showManual = $state(false);
  // §connection-debug: 诊断连接状态，帮助用户理解问题
  let connectionStatus = $state<'idle' | 'connecting' | 'connected' | 'failed'>('idle');
  let connectionDetail = $state('');

  let unsubState: (() => void) | undefined;
  let sessionCheck: AbortController | undefined;
  let alive = true;
  let attempt = 0;

  function submitCode() {
    const numeric = code.replace(/\D/g, '').slice(0, 6);
    if (numeric.length < 6 || loading) return;
    code = '';
    loading = true;
    error = '';
    const current = ++attempt;
    sessionCheck?.abort();
    const check = sessionCheck = new AbortController();

    const host = location.hostname;
    const port = parseInt(location.port) || (location.protocol === 'https:' ? 443 : 80);

    fetch('/verify', {
      signal: check.signal,
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      // §L-3: bind the issued token to this device (in addition to its IP).
      body: `code=${encodeURIComponent(numeric)}&device=${encodeURIComponent(getRemoteDeviceId())}`,
    })
      .then(r => r.json())
      .then(d => {
        if (!alive || current !== attempt) return;
        if (d.success && d.token) {
          writeRemoteToken(d.token);
          connectWithToken(host, port, d.token);
        } else {
          loading = false;
          error = d.message || tr('mobile.invalidCode');
        }
      })
      .catch(() => {
        if (!alive || current !== attempt) return;
        loading = false;
        error = tr('mobile.networkError');
      });
  }

  function connectionFailed(detailOverride?: string) {
    const failure = ws.lastFailure();
    loading = false;
    connectionStatus = 'failed';
    connectionDetail = detailOverride || failure?.message || tr('mobile.connectHostFail');
    showManual = failure?.category === 'user';
    if (showManual) {
      clearRemoteToken();
      error = tr('mobile.authTokenInvalidHint');
    }
  }

  async function connectWithToken(host: string, port: number, token: string) {
    const current = ++attempt;
    unsubState?.();
    sessionCheck?.abort();
    const check = sessionCheck = new AbortController();
    const deadline = setTimeout(() => check.abort(), 5000);
    loading = true;
    showManual = false;
    connectionStatus = 'connecting';
    connectionDetail = tr('mobile.connectingTo', { target: `${host}:${port}` });
    const valid = await remoteSessionValidity(token, check.signal);
    clearTimeout(deadline);
    if (!alive || current !== attempt) return;
    if (valid === false) {
      clearRemoteToken();
      loading = false;
      showManual = true;
      connectionStatus = 'failed';
      connectionDetail = tr('mobile.authTokenInvalidHint');
      return;
    }
    unsubState = ws.onStateChange((state: ConnectionState) => {
      if (!alive || current !== attempt) return;
      if (state === 'connected') {
        loading = false;
        connectionStatus = 'connected';
        connectionDetail = '';
        unsubState?.();
        onverified();
      } else if (state === 'error') {
        connectionFailed();
      }
    });
    ws.connect(host, port, token, 'token');
  }

  function autoReconnect() {
    const token = readRemoteToken();
    if (!token) {
      showManual = true;
      connectionStatus = 'idle';
      connectionDetail = tr('mobile.noSavedToken');
      return;
    }
    const port = parseInt(location.port) || (location.protocol === 'https:' ? 443 : 80);
    void connectWithToken(location.hostname, port, token);
  }

  onMount(() => {
    autoReconnect();
    const focusTimer = setTimeout(() => inputEl?.focus(), 400);
    const resume = () => {
      if (!document.hidden && connectionStatus === 'failed' && !showManual) autoReconnect();
    };
    window.addEventListener('online', resume);
    document.addEventListener('visibilitychange', resume);
    return () => {
      alive = false;
      attempt++;
      sessionCheck?.abort();
      unsubState?.();
      clearTimeout(focusTimer);
      window.removeEventListener('online', resume);
      document.removeEventListener('visibilitychange', resume);
    };
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
    {#if connectionStatus === 'failed' && connectionDetail}
      <div class="connection-error">
        <p class="error-detail">{connectionDetail}</p>
        <p class="error-hint">{$t('mobile.connErrorHint')}</p>
      </div>
    {/if}
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
{:else if connectionStatus === 'failed'}
  <div class="screen" role="alert">
    <h1>Ridge Remote</h1>
    <p class="connection-detail">{connectionDetail}</p>
    <div class="card">
      <button onclick={autoReconnect}>{$t('mobile.refresh')}</button>
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
    {#if connectionDetail}
      <p class="connection-detail">{connectionDetail}</p>
    {/if}
  </div>
{/if}

<style>
  /* Keep reconnect / failure diagnostics in the usable viewport on standalone
     PWA devices.  The auth screen is also the post-reconnect fallback, so it
     needs the same notch contract as MainApp's connection banner.  Overflow is
     intentional: a long host/error detail must remain scrollable instead of
     being clipped behind the notch or a short mobile viewport. */
  .screen{position:fixed;inset:0;background:var(--rg-bg);display:flex;flex-direction:column;align-items:center;justify-content:safe center;padding:24px;box-sizing:border-box;overflow-y:auto;overscroll-behavior:contain}
  @supports (padding-top:constant(safe-area-inset-top)) {
    .screen{padding-top:calc(24px + constant(safe-area-inset-top));padding-right:max(24px,constant(safe-area-inset-right));padding-bottom:calc(24px + constant(safe-area-inset-bottom));padding-left:max(24px,constant(safe-area-inset-left))}
  }
  @supports (padding-top:env(safe-area-inset-top)) {
    .screen{padding-top:calc(24px + env(safe-area-inset-top,0px));padding-right:max(24px,env(safe-area-inset-right,0px));padding-bottom:calc(24px + env(safe-area-inset-bottom,0px));padding-left:max(24px,env(safe-area-inset-left,0px))}
  }
  /* Some standalone Android/WebView shells expose the display cutout while
     returning zero for env(). Keep the retry/login card below a conservative
     portrait top belt; a real inset still wins through max(). */
  @media (display-mode:standalone) and (orientation:portrait) {
    .screen{padding-top:64px}
    @supports (padding-top:constant(safe-area-inset-top)) {
      .screen{padding-top:max(64px,calc(24px + constant(safe-area-inset-top)))}
    }
    @supports (padding-top:env(safe-area-inset-top)) {
      .screen{padding-top:max(64px,calc(24px + env(safe-area-inset-top,0px)))}
    }
  }
  /* iOS standalone exposes navigator.standalone without matching the media
     query; main.ts marks the document before this screen first paints. */
  :global(html[data-ridge-pwa="standalone"]) .screen{padding-top:64px}
  @supports (padding-top:constant(safe-area-inset-top)) {
    :global(html[data-ridge-pwa="standalone"]) .screen{padding-top:max(64px,calc(24px + constant(safe-area-inset-top)))}
  }
  @supports (padding-top:env(safe-area-inset-top)) {
    :global(html[data-ridge-pwa="standalone"]) .screen{padding-top:max(64px,calc(24px + env(safe-area-inset-top,0px)))}
  }
  .logo{display:block;width:64px;height:64px;margin:0 auto 16px}
  h1{font-size:20px;font-weight:600;margin-bottom:4px;color:var(--rg-fg)}
  .sub{color:var(--rg-fg-muted);font-size:14px;margin-bottom:24px;text-align:center;line-height:1.5}
  .card{width:100%;max-width:340px;background:var(--rg-surface);border:1px solid var(--rg-border-bright);border-radius:12px;padding:24px;text-align:center}
  input{width:100%;height:48px;padding:0 16px;border:2px solid var(--rg-border-bright);border-radius:10px;background:var(--rg-bg);color:var(--rg-fg);font-size:24px;font-weight:700;letter-spacing:8px;text-align:center;outline:none;transition:border-color .2s}
  input:focus{border-color:var(--rg-accent)}
  input.has-error{border-color:var(--rg-ansi-red)}
  input::placeholder{color:var(--rg-fg-muted);letter-spacing:2px;font-size:14px}
  .error-msg{color:var(--rg-ansi-red);font-size:13px;margin-top:8px}
  .connection-error{background:var(--rg-surface);border:1px solid var(--rg-ansi-red);border-radius:8px;padding:12px;margin-bottom:16px;text-align:center}
  .connection-error .error-detail{color:var(--rg-ansi-red);font-size:13px;margin:0 0 8px 0;font-weight:500}
  .connection-error .error-hint{color:var(--rg-fg-muted);font-size:12px;margin:0;line-height:1.4}
  .connection-detail{color:var(--rg-fg-muted);font-size:12px;margin-top:8px;text-align:center}
  button{width:100%;height:48px;border:none;border-radius:10px;font-size:16px;font-weight:600;cursor:pointer;transition:opacity .2s;margin-top:16px;background:var(--rg-ansi-green);color:#fff}
  button:disabled{opacity:.4;cursor:not-allowed}
  button:hover:not(:disabled){background:var(--rg-ansi-green)}
</style>
