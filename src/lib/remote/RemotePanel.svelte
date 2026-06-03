<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import QrCode from './QrCode.svelte';
  import { Smartphone, RefreshCw, Power, PowerOff, Wifi, Zap } from 'lucide-svelte';
  import { dev } from '$app/environment';
  import { settingsStore, setSetting } from '$lib/stores/settings';
  import { refreshRemoteRunning } from '$lib/stores/remoteStatus';
  // §cloud: 公网加速（Pro）— 新增并行 provider，不替换 LAN 模式（契约 §9）。
  import CloudProModal from './cloud/CloudProModal.svelte';
  import CloudPanel from './cloud/CloudPanel.svelte';
  import * as cloudAuth from './cloud/auth';
  import { cloudAuth as cloudAuthStore } from './cloud/auth';

  // §cloud: 顶部 Segmented Control 模式。默认 LAN（保留现有全部 UI/逻辑）。
  type RemoteMode = 'lan' | 'cloud';
  let remoteMode = $state<RemoteMode>('lan');
  let proModalOpen = $state(false);

  const cloudState = $derived($cloudAuthStore);
  // Premium 已就绪：已登录 + plan=premium。
  const cloudReady = $derived(cloudAuth.isLoggedIn(cloudState) && cloudAuth.isPremium(cloudState));

  // 切到公网加速：未登录/未订阅则拦截切换并弹 Pro Modal。
  function selectMode(mode: RemoteMode): void {
    if (mode === 'cloud' && !cloudReady) {
      proModalOpen = true; // 拦截，不切换
      return;
    }
    remoteMode = mode;
  }

  // 登录/激活成功后：若已就绪则进入公网加速视图。
  function onCloudReady(): void {
    if (cloudAuth.isLoggedIn(cloudAuth.snapshot()) && cloudAuth.isPremium(cloudAuth.snapshot())) {
      remoteMode = 'cloud';
    }
  }

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

  // §blacklist: persistent device/IP bans (snake_case keys from the Rust struct).
  interface BlacklistDto { id: string; device_id?: string | null; ip?: string | null; label: string; added_at: number; }
  let blacklist = $state<BlacklistDto[]>([]);

  async function refreshBlacklist() {
    try { blacklist = await invoke<BlacklistDto[]>('list_blacklist'); } catch { blacklist = []; }
  }

  async function blacklistSession(id: number) {
    try { await invoke('add_to_blacklist', { id }); } catch { /* ignore */ }
    refreshSessions();
    refreshBlacklist();
  }

  async function unblacklist(id: string) {
    try { await invoke('remove_from_blacklist', { id }); } catch { /* ignore */ }
    refreshBlacklist();
  }

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
    // Dev: the SPA is served by Vite (plain HTTP on :5174), not the Rust
    // server — WebGPU stays Canvas2D in dev, which is fine for UI work.
    // Prod: the Rust server serves HTTPS (self-signed) so browsers get a
    // secure context and the WebGPU render path. First connection per device
    // shows a one-time cert warning to click through.
    if (dev) return `http://${lanIp}:5174/`;
    return `https://${lanIp}:${port}/`;
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
      refreshBlacklist();
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

  <!-- §cloud: Segmented Control — [局域网/自建网] | [官方公网加速 ⚡] -->
  <div class="px-3 pt-3 shrink-0">
    <div class="flex gap-1 rounded-lg bg-[var(--rg-surface)]/60 p-1">
      <button
        onclick={() => selectMode('lan')}
        class="flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-1.5 text-xs font-medium transition-all duration-150
          {remoteMode === 'lan'
            ? 'bg-[var(--rg-accent)]/20 text-[var(--rg-fg)] shadow-sm'
            : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
      >
        <Wifi class="h-3.5 w-3.5" /> 局域网 / 自建网
      </button>
      <button
        onclick={() => selectMode('cloud')}
        class="flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-1.5 text-xs font-medium transition-all duration-150
          {remoteMode === 'cloud'
            ? 'bg-[var(--rg-accent)]/20 text-[var(--rg-fg)] shadow-sm'
            : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
      >
        <Zap class="h-3.5 w-3.5 text-[var(--rg-accent)]" /> 官方公网加速
        {#if !cloudReady}<span class="rounded bg-[var(--rg-accent)]/20 px-1 text-[9px] text-[var(--rg-accent)]">Pro</span>{/if}
      </button>
    </div>
  </div>

  {#if remoteMode === 'cloud'}
    <div class="flex-1 overflow-auto p-3">
      <CloudPanel />
    </div>
  {:else}
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
            <div class="shrink-0 ml-2 flex items-center gap-1">
              <button
                onclick={() => disconnectSession(s.id)}
                class="px-2 py-1 rounded text-[10px] font-medium border border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors"
                title="断开后该设备需重新输入验证码才能连接"
              >
                断开
              </button>
              <button
                onclick={() => blacklistSession(s.id)}
                class="px-2 py-1 rounded text-[10px] font-medium border border-red-500/30 text-red-400 hover:bg-red-500/10 transition-colors"
                title="加入黑名单：失效 token 并禁止重连，直到从黑名单移除"
              >
                拉黑
              </button>
            </div>
          </div>
        {/each}
        {#if sessions.length === 0}
          <p class="text-[11px] text-[var(--rg-fg-muted)] py-1">暂无连接</p>
        {/if}
      </div>

      <!-- §blacklist: barred devices/IPs (persistent) -->
      {#if blacklist.length > 0}
        <div class="bg-[var(--rg-surface)]/50 rounded-lg p-3 space-y-2">
          <h3 class="text-[10px] font-semibold text-[var(--rg-fg-muted)] uppercase tracking-wider">
            黑名单 ({blacklist.length})
          </h3>
          {#each blacklist as b (b.id)}
            <div class="flex items-center justify-between py-1.5 px-2 rounded-md hover:bg-[var(--rg-surface)] transition-colors">
              <div class="min-w-0 flex-1">
                <p class="text-xs text-[var(--rg-fg)] truncate">{b.label}</p>
                <p class="text-[10px] text-[var(--rg-fg-muted)] truncate">
                  {b.device_id ? '设备 ' + b.device_id.slice(0, 8) : ''}{b.device_id && b.ip ? ' · ' : ''}{b.ip ?? ''}
                </p>
              </div>
              <button
                onclick={() => unblacklist(b.id)}
                class="shrink-0 ml-2 px-2 py-1 rounded text-[10px] font-medium border border-[var(--rg-border)] text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/10 transition-colors"
                title="从黑名单移除，允许该设备重新连接"
              >
                移除
              </button>
            </div>
          {/each}
        </div>
      {/if}

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
          {#if !dev}
            <p class="text-[9px] text-amber-400/80 text-center leading-snug max-w-[180px]">
              首次连接会提示「证书不安全」，点「高级 → 继续访问」即可（启用 HTTPS 才能开 WebGPU 加速）
            </p>
          {/if}
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
  {/if}
</div>

<!-- §cloud: Pro 升级 / 登录 Modal（未就绪时拦截切换弹出）-->
<CloudProModal bind:open={proModalOpen} onClose={() => (proModalOpen = false)} onReady={onCloudReady} />
