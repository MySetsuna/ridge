<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { createRemoteConnection, type RemoteConnectionApi } from './wsClient';
  import QrCode from './QrCode.svelte';
  import { Smartphone, Link, Unlink, RefreshCw, Power, PowerOff, ExternalLink } from 'lucide-svelte';
  import { dev } from '$app/environment';

  let remoteEnabled = $state(false);
  let remoteInfo = $state<{ port: number; lanIp: string; totpCode: string; otpauthUri: string; ready: boolean; machineName: string } | null>(null);
  let hostInput = $state('localhost');
  let portInput = $state('');
  let connectError = $state('');
  let totpTimer: ReturnType<typeof setInterval> | null = null;
  let machineName = $state('Ridge');

  import type { RemoteClientEntry } from './wsClient';

  let conn = createRemoteConnection();
  let connected = $state(false);
  let copySuccess = $state(false);
  let remoteClients = $state<RemoteClientEntry[]>([]);
  let clientsTimer: ReturnType<typeof setInterval> | null = null;

  function kickClient(id: number) {
    conn.kickRemoteClient(id);
  }

  function buildLinkUri(lanIp: string, port: number): string {
    if (dev) return `http://${lanIp}:5174/`;
    return `http://${lanIp}:${port}/`;
  }

  async function refreshRemoteInfo() {
    try {
      const info = await invoke<{ port: number; lanIp: string; totpCode: string; otpauthUri: string; ready: boolean; machineName: string }>('get_remote_info');
      remoteInfo = info;
      machineName = info.machineName;
      portInput = String(info.port);
      hostInput = info.lanIp || 'localhost';
    } catch (e: unknown) {
      console.error('Failed to refresh remote info', e);
    }
  }

  import { settingsStore, setSetting } from '$lib/stores/settings';

  async function toggleRemoteEnabled() {
    try {
      const newState = !remoteEnabled;
      await invoke('set_remote_enabled', { enabled: newState });
      remoteEnabled = newState;
      setSetting('remoteEnabled', newState);
      if (newState) {
        await refreshRemoteInfo();
      }
    } catch (e: unknown) {
      connectError = e instanceof Error ? e.message : '切换失败';
    }
  }

  async function connectViaQR() {
    if (!remoteInfo?.ready) return;
    await refreshRemoteInfo();
    if (!remoteInfo?.totpCode) return;
    connected = true;
    conn.connect(hostInput || 'localhost', remoteInfo.port, remoteInfo.totpCode);
  }

  function disconnect() {
    conn.disconnect();
    connected = false;
  }

  async function copyLink() {
    const uri = buildLinkUri(remoteInfo?.lanIp ?? 'localhost', remoteInfo?.port ?? 0);
    try {
      await navigator.clipboard.writeText(uri);
      copySuccess = true;
      setTimeout(() => copySuccess = false, 2000);
    } catch { /* clipbord not available */ }
  }

  $effect(() => {
    const unsub = conn.remoteClients.subscribe(v => remoteClients = v);
    return unsub;
  });

  $effect(() => {
    if (connected) {
      conn.listRemoteClients();
      clientsTimer = setInterval(() => conn.listRemoteClients(), 5000);
    } else {
      if (clientsTimer) { clearInterval(clientsTimer); clientsTimer = null; }
      remoteClients = [];
    }
  });

  onMount(() => {
    refreshRemoteInfo();

    totpTimer = setInterval(async () => {
      if (remoteEnabled) {
        await refreshRemoteInfo();
      }
    }, 5000);
    return () => { if (totpTimer) clearInterval(totpTimer); };
  });

</script>

<div class="flex flex-col h-full">
  <!-- Header with toggle -->
  <div class="flex items-center justify-between px-3 h-10 border-b border-[var(--rg-border)] shrink-0">
    <h2 class="text-xs font-semibold text-[var(--rg-fg)] uppercase tracking-wider flex items-center gap-1.5">
      <Smartphone class="w-3.5 h-3.5" />
      远程控制 ({machineName})
    </h2>
    <div class="flex items-center gap-1">
      {#if connected}
        <button onclick={disconnect} class="p-1 rounded hover:bg-[var(--rg-surface)] transition-colors" title="断开连接">
          <Unlink class="w-3.5 h-3.5 text-red-400" />
        </button>
      {/if}
    </div>
  </div>

  <div class="flex-1 overflow-auto p-3 space-y-4">
    <!-- 启动/停止远程控制 -->
    <div class="flex flex-col items-center gap-2 pt-2 pb-1">
      <button
        onclick={toggleRemoteEnabled}
        class="w-full h-10 rounded-lg font-medium text-sm flex items-center justify-center gap-2 transition-all duration-200 {remoteEnabled
          ? 'bg-green-500/15 text-green-400 border border-green-500/30 hover:bg-green-500/25'
          : 'bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] border border-[var(--rg-border)] hover:border-[var(--rg-accent)]/30 hover:text-[var(--rg-fg)]'}"
      >
        {#if remoteEnabled}
          <Power class="w-4 h-4" />
          远程控制已启用
          <span class="w-2 h-2 rounded-full bg-green-400 animate-pulse"></span>
        {:else}
          <PowerOff class="w-4 h-4" />
          启动远程控制
        {/if}
      </button>
      {#if remoteEnabled}
        <p class="text-[10px] text-[var(--rg-fg-muted)] text-center">
          {#if dev}
            开发模式 · 运行 <code class="bg-[var(--rg-surface)] px-1 rounded">pnpm dev:remote</code> 启动手机端
          {:else}
            手机浏览器扫码或访问
            <button
              onclick={copyLink}
              class="inline bg-transparent border-none p-0 cursor-pointer"
              title="点击复制链接"
            >
              <code class="bg-[var(--rg-surface)] px-1 rounded hover:bg-[var(--rg-accent)]/10 transition-colors">{buildLinkUri(remoteInfo?.lanIp ?? 'localhost', remoteInfo?.port ?? 0)}</code>
            </button>
            <button
              onclick={copyLink}
              class="ml-1 text-[var(--rg-accent)] hover:underline text-[10px]"
              title="复制链接"
            >
              {copySuccess ? '已复制' : '复制'}
            </button>
          {/if}
        </p>
      {/if}
    </div>

    {#if remoteEnabled}
      {#if connected}
        <div class="flex flex-col items-center gap-3 py-4">
          <div class="w-12 h-12 rounded-full bg-green-500/10 flex items-center justify-center">
            <Link class="w-6 h-6 text-green-400" />
          </div>
          <p class="text-sm text-[var(--rg-fg)]">已连接到 {hostInput}:{portInput}</p>
          <p class="text-xs text-[var(--rg-fg-muted)]">远程终端会话活跃中</p>
        </div>

        {#if remoteClients.length > 0}
          <div class="bg-[var(--rg-surface)]/50 rounded-lg p-3 space-y-2">
            <h3 class="text-[10px] font-semibold text-[var(--rg-fg-muted)] uppercase tracking-wider">
              已连接设备 ({remoteClients.length})
            </h3>
            {#each remoteClients as client (client.id)}
              <div class="flex items-center justify-between py-1.5 px-2 rounded-md hover:bg-[var(--rg-surface)] transition-colors">
                <div class="min-w-0 flex-1">
                  <p class="text-xs text-[var(--rg-fg)] truncate">{client.remoteAddr}</p>
                  <p class="text-[10px] text-[var(--rg-fg-muted)]">
                    已连接 {Math.floor(client.connectedAt / 60)} 分
                  </p>
                </div>
                <button
                  onclick={() => kickClient(client.id)}
                  class="shrink-0 ml-2 px-2 py-1 rounded text-[10px] font-medium border border-red-500/30 text-red-400 hover:bg-red-500/10 transition-colors"
                >
                  断开
                </button>
              </div>
            {/each}
          </div>
        {/if}
      {:else if remoteInfo?.ready}
        <!-- QR Code: TOTP authenticator setup -->
        <div class="flex flex-col items-center gap-1 py-1">
          <p class="text-[10px] text-[var(--rg-fg-muted)] mb-1">① 扫码绑定身份验证器</p>
          <QrCode value={remoteInfo.otpauthUri} size={140} />
        </div>

        <!-- QR Code: Link to mobile web page -->
        <div class="flex flex-col items-center gap-1 py-1">
          <p class="text-[10px] text-[var(--rg-fg-muted)] mb-1">② 扫码打开远程页面</p>
          <QrCode value={buildLinkUri(remoteInfo.lanIp, remoteInfo.port)} size={140} />
          <p class="text-[9px] text-[var(--rg-fg-muted)]">手机浏览器扫码 → 输入验证码 → 连接</p>
          <button
            onclick={copyLink}
            class="text-[10px] text-[var(--rg-accent)] hover:underline"
            title="复制链接"
          >
            {copySuccess ? '链接已复制 ✓' : '复制链接'}
          </button>
        </div>

        <!-- Connection info -->
        <div class="bg-[var(--rg-surface)]/50 rounded-lg p-3 space-y-2">
          <div class="flex justify-between text-xs">
            <span class="text-[var(--rg-fg-muted)]">移动端访问入口</span>
            <button onclick={copyLink} class="text-[var(--rg-accent)] font-mono hover:underline cursor-pointer bg-transparent border-none p-0">
              {remoteInfo.lanIp}:{dev ? '5174' : remoteInfo.port}
            </button>
          </div>
          <div class="flex justify-between text-xs">
            <span class="text-[var(--rg-fg-muted)]">后端 WebSocket 端口</span>
            <span class="text-[var(--rg-fg)] font-mono">{remoteInfo.port}</span>
          </div>
          <div class="flex justify-between text-xs">
            <span class="text-[var(--rg-fg-muted)]">TOTP 验证码</span>
            <span class="text-[var(--rg-fg)] font-mono font-bold tracking-wider text-base">{remoteInfo.totpCode}</span>
          </div>
        </div>
      {:else}
        <!-- 正在启动... -->
        <div class="flex flex-col items-center gap-3 py-8 text-center">
          <div class="w-12 h-12 rounded-full bg-[var(--rg-accent)]/10 flex items-center justify-center">
            <RefreshCw class="w-6 h-6 text-[var(--rg-accent)] animate-spin" />
          </div>
          <p class="text-sm text-[var(--rg-fg-muted)]">正在获取远程服务器信息...</p>
        </div>
      {/if}
    {/if}

    {#if connectError}
      <p class="text-xs text-red-400 text-center">{connectError}</p>
    {/if}
  </div>
</div>


