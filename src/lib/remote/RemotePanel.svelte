<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import QrCode from './QrCode.svelte';
  import { Smartphone, RefreshCw, Power, PowerOff } from 'lucide-svelte';
  import { dev } from '$app/environment';
  import { settingsStore, setSetting } from '$lib/stores/settings';
  import { refreshRemoteRunning } from '$lib/stores/remoteStatus';

  // Reflect the persisted/auto-restored state on mount (and stay in sync with
  // the Settings panel, which also reads `settingsStore.remoteEnabled`).
  const remoteEnabled = $derived($settingsStore.remoteEnabled);
  let remoteInfo = $state<{ port: number; lanIp: string; totpCode: string; otpauthUri: string; ready: boolean; machineName: string } | null>(null);
  let connectError = $state('');
  let totpTimer: ReturnType<typeof setInterval> | null = null;
  let machineName = $state('Ridge');
  let copySuccess = $state(false);

  // §sessions: connected remote-control sessions, fetched via Tauri (the desktop
  // has direct AppState access — no need to connect as a WS client). Shown
  // whenever remote control is enabled.
  interface SessionDto { id: number; remoteAddr: string; deviceId: string; userAgent: string; connectedSecs: number; }
  let sessions = $state<SessionDto[]>([]);
  let sessionsTimer: ReturnType<typeof setInterval> | null = null;

  function deviceLabel(s: SessionDto): string {
    if (s.deviceId) return s.deviceId.slice(0, 8);
    return s.remoteAddr || '未知设备';
  }

  async function refreshSessions() {
    try {
      sessions = await invoke<SessionDto[]>('list_remote_sessions');
    } catch {
      sessions = [];
    }
  }

  async function disconnectSession(id: number) {
    try { await invoke('disconnect_session', { id }); } catch { /* ignore */ }
    refreshSessions();
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
    } catch (e: unknown) {
      console.error('Failed to refresh remote info', e);
    }
  }

  async function toggleRemoteEnabled() {
    try {
      const newState = !remoteEnabled;
      await invoke('set_remote_enabled', { enabled: newState });
      setSetting('remoteEnabled', newState);
      await refreshRemoteRunning();
      if (newState) {
        await refreshRemoteInfo();
        await refreshSessions();
      }
    } catch (e: unknown) {
      connectError = e instanceof Error ? e.message : '切换失败';
      void refreshRemoteRunning();
    }
  }

  async function copyLink() {
    const uri = buildLinkUri(remoteInfo?.lanIp ?? 'localhost', remoteInfo?.port ?? 0);
    try {
      await navigator.clipboard.writeText(uri);
      copySuccess = true;
      setTimeout(() => copySuccess = false, 2000);
    } catch { /* clipboard not available */ }
  }

  // §sessions: poll the connected sessions while remote control is enabled.
  $effect(() => {
    if (remoteEnabled) {
      refreshSessions();
      sessionsTimer = setInterval(refreshSessions, 3000);
    } else {
      if (sessionsTimer) { clearInterval(sessionsTimer); sessionsTimer = null; }
      sessions = [];
    }
    return () => { if (sessionsTimer) { clearInterval(sessionsTimer); sessionsTimer = null; } };
  });

  onMount(() => {
    refreshRemoteInfo();
    totpTimer = setInterval(async () => {
      if (remoteEnabled) await refreshRemoteInfo();
    }, 5000);
    return () => { if (totpTimer) clearInterval(totpTimer); };
  });
</script>

<div class="flex flex-col h-full">
  <!-- Header -->
  <div class="flex items-center justify-between px-3 h-10 border-b border-[var(--rg-border)] shrink-0">
    <h2 class="text-xs font-semibold text-[var(--rg-fg)] uppercase tracking-wider flex items-center gap-1.5">
      <Smartphone class="w-3.5 h-3.5" />
      远程控制 ({machineName})
    </h2>
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
            <button onclick={copyLink} class="inline bg-transparent border-none p-0 cursor-pointer" title="点击复制链接">
              <code class="bg-[var(--rg-surface)] px-1 rounded hover:bg-[var(--rg-accent)]/10 transition-colors">{buildLinkUri(remoteInfo?.lanIp ?? 'localhost', remoteInfo?.port ?? 0)}</code>
            </button>
            <button onclick={copyLink} class="ml-1 text-[var(--rg-accent)] hover:underline text-[10px]" title="复制链接">
              {copySuccess ? '已复制' : '复制'}
            </button>
          {/if}
        </p>
      {/if}
    </div>

    {#if remoteEnabled}
      <!-- §sessions: connected devices (live, via Tauri) -->
      <div class="bg-[var(--rg-surface)]/50 rounded-lg p-3 space-y-2">
        <h3 class="text-[10px] font-semibold text-[var(--rg-fg-muted)] uppercase tracking-wider">
          已连接设备 ({sessions.length})
        </h3>
        {#each sessions as s (s.id)}
          <div class="flex items-center justify-between py-1.5 px-2 rounded-md hover:bg-[var(--rg-surface)] transition-colors">
            <div class="min-w-0 flex-1">
              <p class="text-xs text-[var(--rg-fg)] truncate" title={s.deviceId}>{deviceLabel(s)}</p>
              <p class="text-[10px] text-[var(--rg-fg-muted)]">
                {s.remoteAddr} · 已连接 {Math.floor(s.connectedSecs / 60)} 分
              </p>
            </div>
            <button
              onclick={() => disconnectSession(s.id)}
              class="shrink-0 ml-2 px-2 py-1 rounded text-[10px] font-medium border border-red-500/30 text-red-400 hover:bg-red-500/10 transition-colors"
              title="断开后该设备需重新输入验证码才能连接"
            >
              断开
            </button>
          </div>
        {/each}
        {#if sessions.length === 0}
          <p class="text-[11px] text-[var(--rg-fg-muted)] py-1">暂无连接</p>
        {/if}
      </div>

      {#if remoteInfo?.ready}
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
          <button onclick={copyLink} class="text-[10px] text-[var(--rg-accent)] hover:underline" title="复制链接">
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
