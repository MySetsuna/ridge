<script lang="ts">
  import { onMount } from 'svelte';
  import { RemoteConnection, type ConnectionState, getDeviceId } from './lib/wsRemote';

  const TOKEN_KEY = 'ridge_remote_token';

  let { ws, onverified }: { ws: RemoteConnection; onverified: () => void } = $props();

  let code = $state('');
  let error = $state('');
  let loading = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();
  let showManual = $state(false);

  let unsubState: (() => void) | undefined;

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
      body: `code=${encodeURIComponent(numeric)}&device=${encodeURIComponent(getDeviceId())}`,
    })
      .then(r => r.json())
      .then(d => {
        if (d.success && d.token) {
          localStorage.setItem(TOKEN_KEY, d.token);
          connectWithToken(host, port, d.token);
        } else {
          loading = false;
          error = d.message || '验证码无效';
        }
      })
      .catch(() => {
        loading = false;
        error = '网络错误，请重试';
      });
  }

  function connectWithToken(host: string, port: number, token: string) {
    unsubState?.();
    unsubState = ws.onStateChange((s: ConnectionState) => {
      if (s === 'connected') {
        loading = false;
        unsubState?.();
        onverified();
      } else if (s === 'error') {
        loading = false;
        localStorage.removeItem(TOKEN_KEY);
        error = '连接失败，请重新输入验证码';
        showManual = true;
        unsubState?.();
      }
    });
    ws.connect(host, port, token, 'token');
  }

  function autoReconnect() {
    const saved = localStorage.getItem(TOKEN_KEY);
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
    return () => unsubState?.();
  });
</script>

{#if showManual}
  <div class="screen">
    <div class="logo">R</div>
    <h1>Ridge Remote</h1>
    <p class="sub">输入桌面端 Ridge 应用中显示的 6 位动态验证码</p>
    <div class="card">
      <input
        bind:this={inputEl}
        type="text" inputmode="numeric" maxlength={6}
        placeholder="输入 6 位验证码"
        oninput={(e) => { code = (e.target as HTMLInputElement).value.replace(/\D/g, '').slice(0, 6); error = ''; }}
        onkeydown={(e) => { if (e.key === 'Enter') submitCode(); }}
        class:has-error={!!error}
      />
      {#if error}<p class="error-msg">{error}</p>{/if}
      <button onclick={submitCode} disabled={code.length < 6 || loading}>
        {loading ? '验证中...' : '验证并连接'}
      </button>
    </div>
  </div>
{:else if loading}
  <div class="screen">
    <div class="logo">R</div>
    <p class="sub" style="color:#8b949e">正在连接远程桌面...</p>
  </div>
{/if}

<style>
  .screen{position:fixed;inset:0;background:#0d1117;display:flex;flex-direction:column;align-items:center;justify-content:center;padding:24px}
  .logo{width:56px;height:56px;border-radius:16px;background:linear-gradient(135deg,#58a6ff,#1f6feb);display:flex;align-items:center;justify-content:center;margin:0 auto 16px;font-size:24px;font-weight:700;color:#fff}
  h1{font-size:20px;font-weight:600;margin-bottom:4px;color:#e6edf3}
  .sub{color:#8b949e;font-size:14px;margin-bottom:24px;text-align:center;line-height:1.5}
  .card{width:100%;max-width:340px;background:#161b22;border:1px solid #30363d;border-radius:12px;padding:24px;text-align:center}
  input{width:100%;height:48px;padding:0 16px;border:2px solid #30363d;border-radius:10px;background:#0d1117;color:#e6edf3;font-size:24px;font-weight:700;letter-spacing:8px;text-align:center;outline:none;transition:border-color .2s}
  input:focus{border-color:#58a6ff}
  input.has-error{border-color:#f85149}
  input::placeholder{color:#484f58;letter-spacing:2px;font-size:14px}
  .error-msg{color:#f85149;font-size:13px;margin-top:8px}
  button{width:100%;height:48px;border:none;border-radius:10px;font-size:16px;font-weight:600;cursor:pointer;transition:opacity .2s;margin-top:16px;background:#238636;color:#fff}
  button:disabled{opacity:.4;cursor:not-allowed}
  button:hover:not(:disabled){background:#2ea043}
</style>
