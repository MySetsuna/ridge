<script lang="ts">
  import { onMount } from 'svelte';
  import { RemoteConnection, type ConnectionState } from './lib/wsRemote';

  let { ws, onverified }: { ws: RemoteConnection; onverified: () => void } = $props();

  let code = $state('');
  let displayCode = $state('------');
  let error = $state('');
  let loading = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();

  let totpTimer: ReturnType<typeof setInterval> | undefined;
  let unsubState: (() => void) | undefined;

  async function fetchTotp() {
    try {
      const r = await fetch('/info');
      if (r.ok) {
        const d = await r.json();
        displayCode = d.totpCode ?? d.totp_code ?? '------';
      }
    } catch { /* ignore */ }
  }

  function onInput(e: Event) {
    const el = e.target as HTMLInputElement;
    code = el.value.replace(/\D/g, '').slice(0, 6);
    error = '';
  }

  function verify() {
    if (code.length < 6 || loading) return;
    loading = true;
    error = '';

    unsubState?.();
    unsubState = ws.onStateChange((s: ConnectionState) => {
      if (s === 'connected') {
        loading = false;
        clearInterval(totpTimer);
        unsubState?.();
        onverified();
      } else if (s === 'error') {
        loading = false;
        error = '验证失败：验证码无效或服务器拒绝连接';
        unsubState?.();
      }
    });

    ws.connect(location.hostname, parseInt(location.port) || (location.protocol === 'https:' ? 443 : 80), code);
  }

  onMount(() => {
    fetchTotp();
    totpTimer = setInterval(fetchTotp, 5000);
    setTimeout(() => inputEl?.focus(), 300);
    return () => {
      clearInterval(totpTimer);
      unsubState?.();
    };
  });
</script>

<div class="screen">
  <div class="logo">R</div>
  <h1>Ridge Remote</h1>
  <p class="sub">输入身份验证器中的 6 位动态验证码<br />以连接到远程桌面</p>
  <div class="card">
    <div class="totp">{displayCode}</div>
    <p class="hint">当前验证码（每 30 秒刷新）</p>
    <input
      bind:this={inputEl}
      type="text" inputmode="numeric" maxlength={6}
      placeholder="输入 6 位验证码"
      oninput={onInput}
      onkeydown={(e) => { if (e.key === 'Enter' && code.length >= 6) verify(); }}
      class:has-error={!!error}
    />
    {#if error}<p class="error-msg">{error}</p>{/if}
    <button onclick={verify} disabled={code.length < 6 || loading}>
      {loading ? '验证中...' : '验证并连接'}
    </button>
  </div>
</div>

<style>
  .screen{position:fixed;inset:0;background:#0d1117;display:flex;flex-direction:column;align-items:center;justify-content:center;padding:24px}
  .logo{width:56px;height:56px;border-radius:16px;background:linear-gradient(135deg,#58a6ff,#1f6feb);display:flex;align-items:center;justify-content:center;margin:0 auto 16px;font-size:24px;font-weight:700;color:#fff}
  h1{font-size:20px;font-weight:600;margin-bottom:4px;color:#e6edf3}
  .sub{color:#8b949e;font-size:14px;margin-bottom:24px;text-align:center;line-height:1.5}
  .card{width:100%;max-width:340px;background:#161b22;border:1px solid #30363d;border-radius:12px;padding:24px;text-align:center}
  .totp{font-size:36px;font-weight:700;letter-spacing:6px;color:#58a6ff;margin:8px 0;font-variant-numeric:tabular-nums}
  .hint{font-size:12px;color:#8b949e;margin-bottom:16px}
  input{width:100%;height:48px;padding:0 16px;border:2px solid #30363d;border-radius:10px;background:#0d1117;color:#e6edf3;font-size:24px;font-weight:700;letter-spacing:8px;text-align:center;outline:none;transition:border-color .2s}
  input:focus{border-color:#58a6ff}
  input.has-error{border-color:#f85149}
  input::placeholder{color:#484f58;letter-spacing:2px;font-size:14px}
  .error-msg{color:#f85149;font-size:13px;margin-top:8px}
  button{width:100%;height:48px;border:none;border-radius:10px;font-size:16px;font-weight:600;cursor:pointer;transition:opacity .2s;margin-top:16px;background:#238636;color:#fff}
  button:disabled{opacity:.4;cursor:not-allowed}
  button:hover:not(:disabled){background:#2ea043}
</style>
