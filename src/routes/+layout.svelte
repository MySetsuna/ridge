<!-- src/routes/+layout.svelte -->
<script lang="ts">
  import '../app.css';
  import { browser, dev } from '$app/environment';
  import DevIssueDialog from '$lib/components/DevIssueDialog.svelte';
  import { setTransport } from '$lib/transport';
  import { TauriDataProvider } from '$lib/transport/tauri';
  import { onMount } from 'svelte';
  import { t, tr } from '$lib/i18n';

  // §web-remote: when the desktop SPA is served to a plain browser by the LAN
  // remote server, `@tauri-apps/api/*` is aliased to the shims in
  // $lib/transport/tauriShim and this flag is defined `true` by the build (see
  // vite.config.js). In the normal Tauri build the flag is undefined, the whole
  // branch tree-shakes away, and behaviour is unchanged.
  const WEB_REMOTE = import.meta.env.RIDGE_WEB_REMOTE === true;

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
      return;
    }
    // §cloud: 两种方式进入 cloud-controller 模式（优先级从高到低）：
    //   1. URL query: `?cloudHost=<device>&u=<username>`（显式指定）
    //   2. 租户域名: `{device}-{username}.remo2ridge.duckdns.org`（自动从 hostname 解析）
    // 非 cloud 模式则走 LAN TOTP 流程。
    void startCloudControllerBootMode();
  });

  // Cloud controller boot: bootCloudControllerFromUrl tries both URL query
  // params and hostname-based tenant detection; wires the controller WebRTC
  // provider → L1 adapter → bridge → DataProvider internally; flips `ready`
  // once the relay/WebRTC/E2EE handshake reaches `connected`.
  //
  // 回退策略：
  // - 租户域名（`{device}-{username}.remo2ridge.duckdns.org`）上 cloud 接线失败
  //   （无 user token / host 不在线）→ 重定向到 `remo2ridge.duckdns.org` 登录/激活
  // - 主域名上 cloud 接线失败 → 回退 LAN TOTP 流程
  // §跨子域交接（方案 B）：主域登录后经 `#token=<jwt>` 整页回跳到本租户子域。
  // 在 boot 前把 token 落盘到本子域 localStorage，并立即清除 fragment（避免 token
  // 残留在地址栏/历史；fragment 本就不发往服务器，故不进 access log）。
  async function consumeHandoffToken() {
    const hash = location.hash;
    if (!hash || hash.length < 2) return;
    let token: string | null = null;
    try {
      token = new URLSearchParams(hash.slice(1)).get('token');
    } catch {
      token = null;
    }
    if (!token) return;
    try {
      const { persistHandoffToken } = await import('$lib/remote/cloud/auth');
      persistHandoffToken(token);
    } catch { /* ignore */ }
    try {
      history.replaceState(null, '', location.pathname + location.search);
    } catch { /* ignore */ }
  }

  async function startCloudControllerBootMode() {
    const { bootCloudControllerFromUrl, parseCloudControllerHostname } =
      await import('$lib/remote/cloud/cloudControllerBoot');
    // 先消费可能存在的一次性交接 token，再发起 cloud 接线（boot 从 localStorage 读 token）。
    await consumeHandoffToken();
    phase = 'connecting';
    const handle = bootCloudControllerFromUrl(location.search, {
      onState: (s) => {
        if (s === 'connected') {
          // §4 云端 TOTP 二次验证：连上（E2EE 完成）后**先**提示输入 host 展示的
          // 6 位 code，验证通过才标记 ready（驱动桌面 UI）。
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
      // 租户域名上无凭据 / host 不在线 → 回主域名登录。
      if (parseCloudControllerHostname(location.hostname)) {
        window.location.replace(`https://remo2ridge.duckdns.org/?redirect=${encodeURIComponent(location.href)}`);
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
