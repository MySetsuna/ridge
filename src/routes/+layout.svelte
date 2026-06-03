<!-- src/routes/+layout.svelte -->
<script lang="ts">
  import '../app.css';
  import { browser, dev } from '$app/environment';
  import DevIssueDialog from '$lib/components/DevIssueDialog.svelte';
  import { setTransport } from '$lib/transport';
  import { TauriDataProvider } from '$lib/transport/tauri';
  import { onMount } from 'svelte';

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
  let phase = $state('connecting'); // 'connecting' | 'need-code' | 'error'
  let code = $state('');
  let errorMsg = $state('');
  let loading = $state(false);

  onMount(() => {
    if (!WEB_REMOTE) {
      setTransport(new TauriDataProvider());
      return;
    }
    void startWebRemoteBoot();
  });

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
          errorMsg = '连接失败，请重新输入验证码';
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
            errorMsg = d.message || '验证码无效';
          }
        })
        .catch(() => {
          loading = false;
          errorMsg = '网络错误，请重试';
        });
    };
  }

  let submitCode = () => {};

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
      <p class="wr-sub">输入桌面端 Ridge 应用中显示的 6 位动态验证码</p>
      <div class="wr-card">
        <input
          type="text" inputmode="numeric" maxlength={6}
          placeholder="输入 6 位验证码"
          value={code}
          oninput={(e) => { code = e.currentTarget.value.replace(/\D/g, '').slice(0, 6); errorMsg = ''; }}
          onkeydown={(e) => { if (e.key === 'Enter') submitCode(); }}
        />
        {#if errorMsg}<p class="wr-error">{errorMsg}</p>{/if}
        <button onclick={() => submitCode()} disabled={code.length < 6 || loading}>
          {loading ? '验证中...' : '验证并连接'}
        </button>
      </div>
    {:else}
      <p class="wr-sub">正在连接远程桌面...</p>
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
  .wr-card button { width: 100%; height: 48px; border: none; border-radius: 10px; font-size: 16px; font-weight: 600; cursor: pointer; margin-top: 16px; background: var(--rg-ansi-green, #2ea043); color: #fff; }
  .wr-card button:disabled { opacity: .4; cursor: not-allowed; }
</style>
