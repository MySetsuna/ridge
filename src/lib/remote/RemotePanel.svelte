<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { createRemoteConnection, type RemoteConnectionApi } from './wsClient';
  import QrCode from './QrCode.svelte';
  import { Smartphone, Link, Unlink, RefreshCw } from 'lucide-svelte';

  let remoteInfo = $state<{ port: number; totpCode: string; otpauthUri: string; ready: boolean } | null>(null);
  let hostInput = $state('localhost');
  let portInput = $state('');
  let manualCode = $state('');
  let connectError = $state('');

  let conn = createRemoteConnection();
  let connected = $state(false);

  onMount(async () => {
    try {
      const info = await invoke<{ port: number; totpCode: string; otpauthUri: string; ready: boolean }>('get_remote_info');
      remoteInfo = info;
      portInput = String(info.port);
    } catch {
      // Not in Tauri
    }
  });

  async function fetchRemoteInfo() {
    connectError = '';
    try {
      const host = hostInput || 'localhost';
      const port = parseInt(portInput) || 0;
      if (!port) { connectError = '请输入端口号'; return; }
      const res = await fetch(`http://${host}:${port}/info`);
      if (!res.ok) { connectError = `服务器返回 ${res.status}`; return; }
      const data = await res.json();
      remoteInfo = {
        port: data.port || port,
        totpCode: data.totpCode ?? data.totp_code,
        otpauthUri: data.otpauthUri ?? data.otpauth_uri,
        ready: true,
      };
    } catch (e: unknown) {
      connectError = e instanceof Error ? e.message : '连接失败';
    }
  }

  function connectViaQR() {
    if (!remoteInfo?.ready) return;
    connected = true;
    conn.connect(hostInput || 'localhost', remoteInfo.port, remoteInfo.totpCode);
  }

  function connectManually() {
    if (!remoteInfo?.ready || !hostInput || !manualCode) return;
    connected = true;
    conn.connect(hostInput, remoteInfo.port, manualCode);
  }

  function disconnect() {
    conn.disconnect();
    connected = false;
  }
</script>

<div class="flex flex-col h-full">
  <!-- Header -->
  <div class="flex items-center justify-between px-3 h-10 border-b border-[var(--rg-border)] shrink-0">
    <h2 class="text-xs font-semibold text-[var(--rg-fg)] uppercase tracking-wider flex items-center gap-1.5">
      <Smartphone class="w-3.5 h-3.5" />
      远程控制
    </h2>
    {#if connected}
      <button onclick={disconnect} class="p-1 rounded hover:bg-[var(--rg-surface)] transition-colors">
        <Unlink class="w-3.5 h-3.5 text-red-400" />
      </button>
    {/if}
  </div>

  <div class="flex-1 overflow-auto p-3 space-y-4">
    {#if connected}
      <div class="flex flex-col items-center gap-3 py-8">
        <div class="w-12 h-12 rounded-full bg-green-500/10 flex items-center justify-center">
          <Link class="w-6 h-6 text-green-400" />
        </div>
        <p class="text-sm text-[var(--rg-fg)]">已连接到 {hostInput}:{portInput}</p>
        <p class="text-xs text-[var(--rg-fg-muted)]">远程终端会话活跃中</p>
      </div>
    {:else if remoteInfo?.ready}
      <!-- QR Code -->
      <div class="flex flex-col items-center gap-2 py-2">
        <QrCode value={remoteInfo.otpauthUri} size={160} />
        <p class="text-[10px] text-[var(--rg-fg-muted)]">扫码连接此桌面</p>
      </div>

      <div class="flex items-center gap-2">
        <div class="flex-1 h-px bg-[var(--rg-border)]"></div>
        <span class="text-[10px] text-[var(--rg-fg-muted)]">或</span>
        <div class="flex-1 h-px bg-[var(--rg-border)]"></div>
      </div>

      <div class="space-y-2">
        <div class="flex gap-2">
          <input bind:value={hostInput} placeholder="主机" class="flex-1 h-8 px-3 rounded-md bg-[var(--rg-surface)] border border-[var(--rg-border)] text-xs text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)] transition-colors" />
          <input bind:value={portInput} placeholder="端口" class="w-20 h-8 px-2 rounded-md bg-[var(--rg-surface)] border border-[var(--rg-border)] text-xs text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)] transition-colors" />
        </div>
        <button onclick={connectViaQR} class="w-full h-8 rounded-md bg-[var(--rg-accent)] text-white text-xs font-medium transition-opacity flex items-center justify-center gap-1.5">
          <Smartphone class="w-3.5 h-3.5" />
          {remoteInfo.totpCode}
        </button>
      </div>
    {:else}
      <!-- Manual connect -->
      <div class="space-y-2">
        <div class="flex gap-2">
          <input bind:value={hostInput} placeholder="主机地址" class="flex-1 h-8 px-3 rounded-md bg-[var(--rg-surface)] border border-[var(--rg-border)] text-xs text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)] transition-colors" />
          <input bind:value={portInput} placeholder="端口" class="w-20 h-8 px-2 rounded-md bg-[var(--rg-surface)] border border-[var(--rg-border)] text-xs text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)] transition-colors" />
        </div>
        <div class="flex gap-2">
          <input bind:value={manualCode} placeholder="TOTP 验证码" maxlength={6} class="flex-1 h-8 px-3 rounded-md bg-[var(--rg-surface)] border border-[var(--rg-border)] text-xs text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)] transition-colors" />
          <button onclick={connectManually} disabled={!hostInput || manualCode.length < 6} class="shrink-0 h-8 px-3 rounded-md bg-[var(--rg-accent)] text-white text-xs font-medium disabled:opacity-40 transition-opacity">
            连接
          </button>
        </div>
        <button onclick={fetchRemoteInfo} class="w-full h-8 rounded-md border border-dashed border-[var(--rg-border)] text-[var(--rg-fg-muted)] text-xs hover:bg-[var(--rg-surface)] transition-colors flex items-center justify-center gap-1">
          <RefreshCw class="w-3 h-3" />
          获取服务器信息
        </button>
      </div>
    {/if}

    {#if connectError}
      <p class="text-xs text-red-400 text-center">{connectError}</p>
    {/if}
  </div>
</div>

<style>
  input::placeholder { color: var(--rg-fg-muted); opacity: 0.5; }
</style>
