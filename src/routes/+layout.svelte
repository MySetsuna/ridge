<!-- src/routes/+layout.svelte -->
<script lang="ts">
  import '../app.css';
  import { browser, dev } from '$app/environment';
  import DevIssueDialog from '$lib/components/DevIssueDialog.svelte';
  import { setTransport } from '$lib/transport';
  import { TauriDataProvider } from '$lib/transport/tauri';
  import { onMount } from 'svelte';
  import { t, tr } from '$lib/i18n';
  import { invoke } from '@tauri-apps/api/core';
  import { startTotpIdentitySync } from '$lib/remote/totpIdentitySync';
  import { cloudAuth as cloudAuthStore } from '$lib/remote/cloud/auth';
  import { BASE_DOMAIN, cloudHttpScheme } from '$lib/remote/cloud/apiClient';

  // §web-remote: when the desktop SPA is served to a plain browser by the LAN
  // remote server, `@tauri-apps/api/*` is aliased to the shims in
  // $lib/transport/tauriShim and this flag is defined `true` by the build (see
  // vite.config.js). In the normal Tauri build the flag is undefined, the whole
  // branch tree-shakes away, and behaviour is unchanged.
  const WEB_REMOTE = import.meta.env.RIDGE_WEB_REMOTE === true;

  // §redirect-loop 止血：租户子域 boot 失败回主域登录的「已回跳」计数（per-tab，
  // sessionStorage 跨子域↔主域同标签往返保留）。第二次仍失败即停在子域显式报错，
  // 不再无限回跳（apex⇄子域死循环的客户端一端）。connected 时清零。
  const TENANT_BOUNCE_KEY = 'ridge_tenant_login_bounce';

  // Auth/connect state for the web-remote gate. `ready` blocks the page outlet
  // until the bridge is attached, so the desktop UI never calls `invoke()`
  // before the WS is live.
  let { children } = $props();
  let ready = $state(!WEB_REMOTE);
  let phase = $state('connecting'); // 'connecting' | 'need-code' | 'need-totp' | 'error'
  let code = $state('');
  let errorMsg = $state('');
  let loading = $state(false);
  // JOB1: when the verify fetch fails opaquely on an HTTPS origin we surface a
  // cert-trust hint + a "/ridge-ca.crt" download link instead of a bare
  // "网络错误". An opaque `fetch` rejection on a self-signed HTTPS origin is
  // almost always ERR_CERT_* — the same-origin call is blocked even though the
  // user clicked through the page-load warning. The browser never exposes the
  // cert reason to JS, so we infer it from the protocol.
  let showCertHint = $state(false);

  onMount(() => {
    if (!WEB_REMOTE) {
      setTransport(new TauriDataProvider());
      // §totp-persist：仅真实桌面 host 同步登录态→TOTP 种子（web-remote 已被
      // WEB_REMOTE 分支排除，不会到这）。
      const stopTotpSync = startTotpIdentitySync(invoke, cloudAuthStore);
      return () => stopTotpSync();
    }
    // §cloud: 两种方式进入 cloud-controller 模式（优先级从高到低）：
//   1. URL query: `?cloudHost=<device>&u=<username>`（显式指定）
  //   2. 租户域名: `{device}-{username}.9527127.xyz`（自动从 hostname 解析）
  // 非 cloud 模式则走 LAN TOTP 流程。
    void startCloudControllerBootMode();
  });

  // Cloud controller boot: bootCloudControllerFromUrl tries both URL query
  // params and hostname-based tenant detection; wires the controller WebRTC
  // provider → L1 adapter → bridge → DataProvider internally; flips `ready`
  // once the relay/WebRTC/E2EE handshake reaches `connected`.
  //
  // 回退策略：
  // - 租户域名（`{device}-{username}.9527127.xyz`）上 cloud 接线失败
  //   （无 user token / host 不在线）→ 重定向到 `9527127.xyz` 登录/激活
  // - 主域名上 cloud 接线失败 → 回退 LAN TOTP 流程
  async function startCloudControllerBootMode() {
    const { bootCloudControllerFromUrl, parseCloudControllerHostname } =
      await import('$lib/remote/cloud/cloudControllerBoot');
    // 父域 cookie bootstrap（设计 2026-06-12-cloud-domain-sso）：用父域 `ridge_sso` cookie
    // 换短 access token、seed 登录态（替代旧 `#token` 跨子域握手）。失败仅返回 false，由下方
    // boot 失败回退（租户子域回主域登录 / 主域回退 LAN）统一处理。
    const { bootstrapFromCookie } = await import('$lib/remote/cloud/auth');
    // 返回值 = 父域 ridge_sso cookie 是否有效（成功换出 access token）。失败 = cookie 缺失/失效。
    const hadSession = await bootstrapFromCookie();
    phase = 'connecting';
    const handle = bootCloudControllerFromUrl(location.search, {
      onState: (s) => {
        if (s === 'connected') {
          // §4 云端 TOTP 二次验证：连上（E2EE 完成）后**先**提示输入 host 展示的
          // 6 位 code，验证通过才标记 ready（驱动桌面 UI）。
          // 接线成功（鉴权 + WebRTC OK）→ 清掉回跳计数，下次刷新从零开始。
          try { sessionStorage.removeItem(TENANT_BOUNCE_KEY); } catch { /* ignore */ }
          phase = 'need-totp';
          errorMsg = '';
          loading = false;
        } else if (s === 'error') {
          phase = 'error';
          errorMsg = errorMsg || tr('main.remoteGateErrCloud');
        }
      },
      onError: (msg) => { phase = 'error'; errorMsg = msg; },
    }, location.hostname);
    cloudHandle = handle;
    if (!handle) {
      // 租户域名上接线失败 → (重新)登录拿 cookie；但要防 apex⇄子域死循环。
      if (parseCloudControllerHostname(location.hostname)) {
        // bootCloudControllerFromUrl 仅在缺 userToken/username（即 cookie 无效）时返回 null；
        // host 离线是返回句柄后经 onState('error')，不会到这。
        // 止血：①已回跳过一次仍失败，或 ②本就有有效 cookie 却仍接线失败（多半是 host 离线/
        // 未设用户名而非鉴权）→ 停在子域显式报错，别再无限回跳。
        let bounced = 0;
        try { bounced = parseInt(sessionStorage.getItem(TENANT_BOUNCE_KEY) || '0', 10) || 0; } catch { /* ignore */ }
        if (bounced >= 1 || hadSession) {
          try { sessionStorage.removeItem(TENANT_BOUNCE_KEY); } catch { /* ignore */ }
          phase = 'error';
          errorMsg = tr('main.remoteGateErrTenantLoginStuck');
          return;
        }
        try { sessionStorage.setItem(TENANT_BOUNCE_KEY, String(bounced + 1)); } catch { /* ignore */ }
        // 回主域名登录/激活。用配置的 BASE_DOMAIN（debug 包烘焙为 localhost:5001），
        // 不再硬编码生产域名——否则 dev 下租户子域 boot 失败会被踢去生产站。
        const scheme = cloudHttpScheme(BASE_DOMAIN);
        window.location.replace(`${scheme}://${BASE_DOMAIN}/?redirect=${encodeURIComponent(location.href)}`);
        return;
      }
      // 主域名上非 cloud 模式 → 回退 LAN TOTP。
      void startWebRemoteBoot();
    }
  }

  async function startWebRemoteBoot() {
    const { RemoteConnection } = await import('../remote/lib/wsRemote');
    const { bridge } = await import('$lib/transport/tauriShim/bridge');
    const { createLanWsTransport } = await import('$lib/transport/remote/lanWsAdapter');
    const TOKEN_KEY = 'ridge_remote_token';
    const conn = new RemoteConnection();

    const host = location.hostname;
    const port = parseInt(location.port) || (location.protocol === 'https:' ? 443 : 80);

    const finish = () => {
      // Wrap the authenticated RemoteConnection in the L1 LAN-WS adapter; the
      // bridge depends on the transport interface (L2 RPC + L1 pane bytes), not
      // on RemoteConnection directly (handoff plan §5.3, D6/D7).
      bridge.attach(createLanWsTransport(conn));
      // DataProvider consumers (FS/git/search) ride the same shimmed invoke.
      setTransport(new TauriDataProvider());
      ready = true;
      // §弱网: register the SW so the desktop bundle + Monaco are served from
      // local cache; only data crosses the WS thereafter.
      if ('serviceWorker' in navigator) {
        // Classic script: SvelteKit bundles the SW self-contained for production.
        navigator.serviceWorker.register('/service-worker.js').catch(() => {});
      }
    };

    const connectWith = (token: string) => {
      loading = true;
      errorMsg = '';
      const unsub = conn.onStateChange((s) => {
        if (s === 'connected') {
          loading = false;
          unsub();
          finish();
        } else if (s === 'error') {
          loading = false;
          unsub();
          try { localStorage.removeItem(TOKEN_KEY); } catch { /* ignore */ }
          phase = 'need-code';
          errorMsg = tr('main.remoteGateErrReconnect');
        }
      });
      conn.connect(host, port, token, 'token');
    };

    // Try saved token first; fall back to the 6-digit TOTP prompt.
    let saved = null;
    try { saved = localStorage.getItem(TOKEN_KEY); } catch { /* ignore */ }
    if (saved) {
      connectWith(saved);
    } else {
      phase = 'need-code';
    }

    // Expose the submit handler to the template via closure.
    submitCode = () => {
      const numeric = code.replace(/\D/g, '').slice(0, 6);
      if (numeric.length < 6 || loading) return;
      code = '';
      loading = true;
      errorMsg = '';
      showCertHint = false;
      fetch('/verify', {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: `code=${encodeURIComponent(numeric)}`,
      })
        .then((r) => r.json())
        .then((d) => {
          if (d.success && d.token) {
            try { localStorage.setItem(TOKEN_KEY, d.token); } catch { /* ignore */ }
            connectWith(d.token);
          } else {
            loading = false;
            errorMsg = d.message || tr('main.remoteGateErrInvalidCode');
          }
        })
        .catch(() => {
          loading = false;
          // JOB1 root cause: `fetch('/verify')` is SAME-ORIGIN against the remote
          // server (this bundle is only ever served by server.rs over its own
          // HTTPS origin, never by Vite). An opaque rejection here on an https
          // page is therefore almost always the self-signed cert being
          // untrusted (ERR_CERT_*) — not a true network outage. Surface the
          // cert-trust hint + CA download instead of a confusing "网络错误".
          if (location.protocol === 'https:') {
            showCertHint = true;
            errorMsg = tr('main.remoteGateErrCert');
          } else {
            errorMsg = tr('main.remoteGateErrNetwork');
          }
        });
    };
  }

  let submitCode = () => {};

  // §4 云端 TOTP：boot 句柄（连上后用于经 CONTROL 通道发码验证）。
  let cloudHandle: import('$lib/remote/cloud/cloudControllerBoot').CloudControllerHandle | null = null;

  // §4 controller 端 TOTP 提交：把 6 位 code 经 CONTROL 通道发给 host 验证；
  // ok → 标记 ready（放行桌面 UI）；fail/超时 → 错误提示 + 允许重试。
  function submitTotp() {
    const numeric = code.replace(/\D/g, '').slice(0, 6);
    if (numeric.length < 6 || loading || !cloudHandle) return;
    loading = true;
    errorMsg = '';
    cloudHandle
      .verifyTotp(numeric)
      .then((ok) => {
        loading = false;
        if (ok) {
          code = '';
          ready = true;
          if ('serviceWorker' in navigator) {
            navigator.serviceWorker.register('/service-worker.js').catch(() => {});
          }
        } else {
          code = '';
          errorMsg = tr('main.totpGateErrInvalid');
        }
      })
      .catch(() => {
        loading = false;
        code = '';
        errorMsg = tr('main.totpGateErrNetwork');
      });
  }

  // §A.7 (2026-05-08): the @fontsource/noto-color-emoji webfont was
  // removed — WebView2 / Chromium versions in the Tauri runtime fail
  // to render Noto's COLRv1 outlines via canvas `fillText`. Removing
  // the bundled webfont lets the font-family stack fall through cleanly
  // to the system emoji fonts, which are reliable across all platforms.
</script>

{#if ready}
  <div class="min-h-screen min-h-[100dvh] bg-[var(--rg-bg)] text-[var(--rg-fg)] antialiased">
    {@render children()}
  </div>
{:else}
  <div class="wr-gate">
    {#if phase === 'need-code'}
      <h1>Ridge Remote</h1>
      <p class="wr-sub">{$t('main.remoteGateSubtitle')}</p>
      <div class="wr-card">
        <input
          type="text" inputmode="numeric" maxlength={6}
          placeholder={$t('main.remoteGatePlaceholder')}
          value={code}
          oninput={(e) => { code = e.currentTarget.value.replace(/\D/g, '').slice(0, 6); errorMsg = ''; }}
          onkeydown={(e) => { if (e.key === 'Enter') submitCode(); }}
        />
        {#if errorMsg}<p class="wr-error">{errorMsg}</p>{/if}
        <!-- JOB1: cert-trust escape hatch — `/ridge-ca.crt` is the public CA
             served by server.rs (no token). Trusting it once silences the
             self-signed warning that was blocking the same-origin verify fetch. -->
        {#if showCertHint}
          <a class="wr-trust" href="/ridge-ca.crt" download="ridge-remote-ca.crt">
            {$t('main.remoteGateTrustCert')}
          </a>
        {/if}
        <button onclick={() => submitCode()} disabled={code.length < 6 || loading}>
          {loading ? $t('main.remoteGateVerifying') : $t('main.remoteGateConnect')}
        </button>
      </div>
    {:else if phase === 'need-totp'}
      <!-- §4 云端 TOTP 二次验证：连上后输入 host（桌面端 Cloud tab）展示的 6 位 code。 -->
      <h1>Ridge Remote</h1>
      <p class="wr-sub">{$t('main.totpGateSubtitle')}</p>
      <div class="wr-card">
        <input
          type="text" inputmode="numeric" maxlength={6}
          placeholder={$t('main.remoteGatePlaceholder')}
          value={code}
          oninput={(e) => { code = e.currentTarget.value.replace(/\D/g, '').slice(0, 6); errorMsg = ''; }}
          onkeydown={(e) => { if (e.key === 'Enter') submitTotp(); }}
        />
        {#if errorMsg}<p class="wr-error">{errorMsg}</p>{/if}
        <button onclick={() => submitTotp()} disabled={code.length < 6 || loading}>
          {loading ? $t('main.remoteGateVerifying') : $t('main.totpGateVerify')}
        </button>
      </div>
    {:else}
      <p class="wr-sub">{$t('main.remoteGateConnecting')}</p>
      {#if errorMsg}<p class="wr-error">{errorMsg}</p>{/if}
    {/if}
  </div>
{/if}

{#if dev && browser && !WEB_REMOTE}
  <DevIssueDialog />
{/if}

<style>
  .wr-gate { position: fixed; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 24px; background: var(--rg-bg, #0d1117); color: var(--rg-fg, #e6edf3); }
  .wr-gate h1 { font-size: 20px; font-weight: 600; margin-bottom: 4px; }
  .wr-sub { color: var(--rg-fg-muted, #8b949e); font-size: 14px; margin-bottom: 24px; text-align: center; }
  .wr-card { width: 100%; max-width: 340px; background: var(--rg-surface, #161b22); border: 1px solid var(--rg-border-bright, #30363d); border-radius: 12px; padding: 24px; text-align: center; }
  .wr-card input { width: 100%; height: 48px; padding: 0 16px; border: 2px solid var(--rg-border-bright, #30363d); border-radius: 10px; background: var(--rg-bg, #0d1117); color: var(--rg-fg, #e6edf3); font-size: 24px; font-weight: 700; letter-spacing: 8px; text-align: center; outline: none; }
  .wr-card input:focus { border-color: var(--rg-accent, #7fb069); }
  .wr-error { color: var(--rg-ansi-red, #f85149); font-size: 13px; margin-top: 8px; }
  .wr-trust { display: inline-block; margin-top: 12px; font-size: 13px; color: var(--rg-accent, #7fb069); text-decoration: underline; cursor: pointer; }
  .wr-card button { width: 100%; height: 48px; border: none; border-radius: 10px; font-size: 16px; font-weight: 600; cursor: pointer; margin-top: 16px; background: var(--rg-ansi-green, #2ea043); color: #fff; }
  .wr-card button:disabled { opacity: .4; cursor: not-allowed; }
</style>
