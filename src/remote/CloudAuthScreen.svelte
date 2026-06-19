<script lang="ts">
  // Cloud auth gate for the mobile app (design 2026-06-16-mobile-cloud).
  //
  // Mirrors the desktop web-remote cloud boot in src/routes/+layout.svelte, but
  // produces a CloudRemoteConnection for the mobile MainApp instead of flipping a
  // `ready` flag for the desktop SPA. Flow (contract §2.3 / §4 / §7):
  //   1. bootstrapFromCookie() — parent-domain ridge_sso cookie → user JWT.
  //   2. bootCloudControllerFromUrl() — signaling + WebRTC + §7.1 E2EE handshake.
  //   3. on 'connected' → prompt the §4 zero-trust TOTP (host shows the 6-digit code).
  //   4. handle.verifyTotp() over the E2EE CONTROL channel → construct + init the
  //      cloud transport → hand it up to App.svelte.
  // This is the SAME asymmetric-E2EE + zero-trust path the desktop browser uses;
  // the mobile now rides it with its own UI.
  import { onMount } from 'svelte';
  import { t, tr } from '$lib/i18n';
  import { setTransport } from '$lib/transport';
  import { TauriDataProvider } from '$lib/transport/tauri';
  import { CloudRemoteConnection } from './lib/cloudRemote';
  import type { CloudControllerHandle } from '$lib/remote/cloud/cloudControllerBoot';

  let { onready, onfallbacklan }: {
    onready: (conn: CloudRemoteConnection) => void;
    onfallbacklan: () => void;
  } = $props();

  // §redirect-loop 止血（与 +layout.svelte 同源）：租户子域无有效会话→回主域登录的
  // per-tab 计数，防 apex⇄子域死循环。
  const TENANT_BOUNCE_KEY = 'ridge_tenant_login_bounce';

  let phase = $state<'connecting' | 'need-totp' | 'error'>('connecting');
  let code = $state('');
  let error = $state('');
  let loading = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();
  let handle: CloudControllerHandle | null = null;
  // Set once the TOTP gate passes. While null, the boot's onState drives THIS gate;
  // once set, ongoing provider state (drop / reconnect / error) is forwarded to the
  // live transport so MainApp shows link status + auto-reconnects (this closure
  // survives CloudAuthScreen unmount — the provider keeps the reference alive).
  let cloudConn: CloudRemoteConnection | null = null;

  onMount(() => { void boot(); });

  async function boot() {
    const mod = await import('$lib/remote/cloud/cloudControllerBoot');
    const { bootstrapFromCookie } = await import('$lib/remote/cloud/auth');

    // Defensive: App.svelte routed us here on a loose hostname heuristic. If the
    // strict §1.1/§1.2 tenant parse says this is NOT a tenant host (and there's no
    // ?cloudHost=), fall back to the LAN flow rather than booting cloud on a LAN host.
    const isTenant =
      mod.parseCloudControllerUrl(location.search) ||
      mod.parseCloudControllerHostname(location.hostname);
    if (!isTenant) { onfallbacklan(); return; }

    const hadSession = await bootstrapFromCookie();
    phase = 'connecting';
    handle = mod.bootCloudControllerFromUrl(
      location.search,
      {
        onState: (s) => {
          // Post-gate: hand ongoing state to the live transport (drop / reconnect).
          if (cloudConn) { cloudConn.notifyState(s); return; }
          // Pre-gate: this is the initial connect driving the TOTP prompt.
          if (s === 'connected') {
            // E2EE handshake done — clear the bounce counter, prompt zero-trust TOTP.
            try { sessionStorage.removeItem(TENANT_BOUNCE_KEY); } catch { /* ignore */ }
            phase = 'need-totp';
            error = '';
            loading = false;
            setTimeout(() => inputEl?.focus(), 300);
          } else if (s === 'error') {
            phase = 'error';
            error = error || tr('main.remoteGateErrCloud');
          }
        },
        onError: (msg, code) => {
          // Post-gate: 把服务端「已认证但无权」的稳定 code 转发给 live transport，让它
          // 分级（用户问题 / 设备停用 / 通道）并驱动 MainApp 的 banner + 退回登录逻辑。
          if (cloudConn) { cloudConn.notifyError(msg, code); return; }
          phase = 'error';
          error = msg;
        },
      },
      location.hostname,
    );

    if (!handle) {
      // bootCloudControllerFromUrl returns null only when user token / username is
      // missing (cookie invalid). Redirect to the main-domain login (bounce-guarded
      // so a stuck session doesn't loop apex⇄subdomain forever).
      const { BASE_DOMAIN, cloudHttpScheme } = await import('$lib/remote/cloud/apiClient');
      let bounced = 0;
      try { bounced = parseInt(sessionStorage.getItem(TENANT_BOUNCE_KEY) || '0', 10) || 0; } catch { /* ignore */ }
      if (bounced >= 1 || hadSession) {
        try { sessionStorage.removeItem(TENANT_BOUNCE_KEY); } catch { /* ignore */ }
        phase = 'error';
        error = tr('main.remoteGateErrTenantLoginStuck');
        return;
      }
      try { sessionStorage.setItem(TENANT_BOUNCE_KEY, String(bounced + 1)); } catch { /* ignore */ }
      const scheme = cloudHttpScheme(BASE_DOMAIN);
      window.location.replace(`${scheme}://${BASE_DOMAIN}/?redirect=${encodeURIComponent(location.href)}`);
    }
  }

  function submitTotp() {
    const numeric = code.replace(/\D/g, '').slice(0, 6);
    if (numeric.length < 6 || loading || !handle) return;
    loading = true;
    error = '';
    handle
      .verifyTotp(numeric)
      .then(async (ok) => {
        if (!ok) {
          loading = false;
          code = '';
          error = tr('main.totpGateErrInvalid');
          return;
        }
        // Zero-trust TOTP passed → the cloud session is fully authorized. The boot
        // already wired TauriDataProvider for FS/git/search; reassert it defensively
        // (idempotent) so the sidebar rides the same shimmed invoke.
        setTransport(new TauriDataProvider());
        const conn = new CloudRemoteConnection(handle!);
        conn.setVerifiedCode(numeric); // cached for transparent re-auth on full reconnect
        cloudConn = conn; // route ongoing provider state (drop/reconnect) to the transport
        await conn.init();
        loading = false;
        code = '';
        onready(conn);
      })
      .catch(() => {
        loading = false;
        code = '';
        error = tr('main.totpGateErrNetwork');
      });
  }
</script>

{#if phase === 'need-totp'}
  <div class="screen">
    <svg class="logo" viewBox="0 0 32 32" fill="none" aria-label="Ridge mark">
      <rect x="2.5" y="2.5" width="27" height="27" rx="6" stroke="#7fb069" stroke-width="2"/>
      <line x1="16" y1="3.5" x2="16" y2="28.5" stroke="#7fb069" stroke-width="2"/>
      <line x1="3.5" y1="16" x2="28.5" y2="16" stroke="#7fb069" stroke-width="2"/>
      <rect x="4.5" y="4.5" width="9.5" height="9.5" rx="2" fill="#7fb069" fill-opacity="0.18"/>
      <rect x="18" y="18" width="9.5" height="9.5" rx="2" fill="#d97757" fill-opacity="0.22"/>
    </svg>
    <h1>Ridge Remote</h1>
    <p class="sub">{$t('main.totpGateSubtitle')}</p>
    <div class="card">
      <input
        bind:this={inputEl}
        type="text" inputmode="numeric" maxlength={6}
        placeholder={$t('main.remoteGatePlaceholder')}
        value={code}
        oninput={(e) => { code = (e.target as HTMLInputElement).value.replace(/\D/g, '').slice(0, 6); error = ''; }}
        onkeydown={(e) => { if (e.key === 'Enter') submitTotp(); }}
        class:has-error={!!error}
      />
      {#if error}<p class="error-msg">{error}</p>{/if}
      <button onclick={submitTotp} disabled={code.length < 6 || loading}>
        {loading ? $t('mobile.verifying') : $t('main.totpGateVerify')}
      </button>
    </div>
  </div>
{:else}
  <div class="screen">
    <svg class="logo" viewBox="0 0 32 32" fill="none" aria-label="Ridge mark">
      <rect x="2.5" y="2.5" width="27" height="27" rx="6" stroke="#7fb069" stroke-width="2"/>
      <line x1="16" y1="3.5" x2="16" y2="28.5" stroke="#7fb069" stroke-width="2"/>
      <line x1="3.5" y1="16" x2="28.5" y2="16" stroke="#7fb069" stroke-width="2"/>
      <rect x="4.5" y="4.5" width="9.5" height="9.5" rx="2" fill="#7fb069" fill-opacity="0.18"/>
      <rect x="18" y="18" width="9.5" height="9.5" rx="2" fill="#d97757" fill-opacity="0.22"/>
    </svg>
    <p class="sub">{phase === 'error' ? error : $t('main.remoteGateConnecting')}</p>
    {#if phase === 'error' && error}<p class="error-msg">{error}</p>{/if}
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
