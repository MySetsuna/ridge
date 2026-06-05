<script lang="ts">
  // Ridge Cloud — 公网加速面板（Premium 已就绪态）。
  //
  // 展示专属域名、连接状态、设备激活/连接控制，以及「进入深根模式 🌱」按钮。
  // 与 Deep Root agent 的跨 agent 命令契约（契约 §8.1）：
  //   - 连接建立/断开 → invoke('set_cloud_remote_active', { active })
  //   - 深根按钮       → invoke('enter_deep_root_mode')
  //   命令暂不存在时用 try/catch 容错，不报错。

  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { Globe, Wifi, WifiOff, Loader2, Power, Plus, ExternalLink } from 'lucide-svelte';
  import QrCode from '../QrCode.svelte';
  import MinimizeButton from '../MinimizeButton.svelte';
  import * as auth from './auth';
  import { cloudAuth } from './auth';
  import { ApiError } from './apiClient';
  import { RidgeCloudProvider } from './ridgeCloudProvider';
  import { CloudHostBridge } from './cloudHostBridge';
  import { createCloudPaneSource } from './cloudPaneSource';
  import type { CloudConnectionState } from './connectionProvider';
  import { t, tr } from '$lib/i18n';

  const authState = $derived($cloudAuth);
  const domain = $derived(auth.publicEntryDomain(authState));
  const hasDevice = $derived(!!authState.deviceToken && !!authState.deviceName);

  let provider: RidgeCloudProvider | null = null;
  // host=answerer 应用层桥：把 provider 解密后的明文帧 demux → 本地执行 invoke
  // / $/hello 协商 / pane 流推回（契约 §5.1/§7）。
  let hostBridge: CloudHostBridge | null = null;
  let connState = $state<CloudConnectionState>('disconnected');
  let connError = $state('');

  // §4 云端 TOTP 展示：复用 LAN 的 get_remote_info（同一本机 RemoteAuth → 同一
  // 6 位 code + otpauth URI），仅在 cloud 会话活跃时展示，供用户读出输入到 controller。
  let remoteInfo = $state<{ totpCode: string; otpauthUri: string } | null>(null);
  let totpTimer: ReturnType<typeof setInterval> | null = null;

  async function refreshRemoteInfo(): Promise<void> {
    try {
      const info = await invoke<{ totpCode: string; otpauthUri: string }>('get_remote_info');
      remoteInfo = { totpCode: info.totpCode, otpauthUri: info.otpauthUri };
    } catch {
      /* 桌面端命令缺失（如 web-remote 环境）时容错忽略 */
    }
  }

  // 在默认浏览器打开该设备专属子域（controller 入口，契约 §3 流程第 5 步）。
  // 打开后用户在子域页面输入下方 TOTP 即可真正控制本机。opener 缺失时回退 window.open。
  async function openPublicRemote(): Promise<void> {
    if (!domain) return;
    const url = `https://${domain}`;
    try {
      const opener = await import('@tauri-apps/plugin-opener');
      await opener.openUrl(url);
    } catch {
      try {
        window.open(url, '_blank', 'noopener');
      } catch {
        /* 无可用打开方式时静默（避免抛到 UI） */
      }
    }
  }

  // 设备激活
  let activating = $state(false);
  let deviceNameInput = $state('');
  let pairingHint = $state('');

  const stateLabel = $derived<Record<CloudConnectionState, string>>({
    disconnected: $t('cloud.stateDisconnected'),
    connecting: $t('cloud.stateConnecting'),
    handshaking: $t('cloud.stateHandshaking'),
    connected: $t('cloud.stateConnected'),
    error: $t('cloud.stateError'),
  });

  function codeToMessage(code: string): string {
    const msg = tr(`errors.${code}`);
    return msg === `errors.${code}` ? tr('errors.GENERIC') : msg;
  }

  // 跨 agent 命令：通知 Rust 侧云端远控活跃状态（契约 §8.1）。容错。
  async function notifyCloudActive(active: boolean): Promise<void> {
    try {
      await invoke('set_cloud_remote_active', { active });
    } catch {
      /* 命令可能尚未由 Deep Root agent 实现，容错忽略 */
    }
  }

  async function activateDevice(): Promise<void> {
    connError = '';
    activating = true;
    pairingHint = '';
    try {
      await auth.activateThisDevice(deviceNameInput.trim(), (p) => {
        pairingHint = tr('cloud.pairingHint', { code: p.pairingCode, sec: p.expiresIn });
      });
      pairingHint = '';
    } catch (e) {
      connError = e instanceof ApiError ? codeToMessage(e.code) : tr('cloud.errActivateFailed');
    } finally {
      activating = false;
    }
  }

  async function connect(): Promise<void> {
    connError = '';
    const s = auth.snapshot();
    if (!s.deviceToken || !s.deviceName || !s.user?.username) {
      connError = tr('cloud.errDeviceNotActivated');
      return;
    }
    // 先建 host 桥：sendFrame 闭包延迟读取 provider（构造完成后再被调用）。
    // host 是 Tauri 桌面 app → 注入真实 `invoke` 执行本地命令（契约 §0/§5.1）。
    const bridge = new CloudHostBridge({
      invoke: (method, params) => invoke(method, params),
      sendFrame: (plaintext) => provider?.sendFrame(plaintext),
      // pane 流接入点（D-GM-11）：host 跑在 WebView，webview 本就经 Tauri event
      // `pty-output-{ws}-{pane}` 收到与 LAN `RawBytes` 同源的裸 PTY 字节。本源订阅
      // 该 event、把 payload.data 编回字节经 onOutput 推出 → 桥编 0x10 发回 controller
      // （controller 端走与 LAN 一致的 onPaneBytes→kernel.feed）。纯前端，不动 Rust。
      paneOutputSource: createCloudPaneSource({
        listen,
        getActiveWorkspaceId: () => invoke<string>('get_active_workspace_id'),
      }),
      // §4 云端 TOTP 二次验证：注入本机 RemoteAuth 校验（与 LAN 同源 RFC6238，
      // ±1 窗口）。controller 经 CONTROL 通道发来 6 位 code，host 桥在放行业务帧
      // 前调此命令；未通过则拒绝 invoke/pane。
      totpVerifier: (code) => invoke<boolean>('verify_remote_totp', { code }),
      // keyBindingVerifier：§5.5 公钥↔设备身份绑定，待 cloud 后端提供带外校验通道后注入。
    });
    hostBridge = bridge;

    provider = new RidgeCloudProvider(
      { deviceToken: s.deviceToken, username: s.user.username },
      {
        onState: (st) => { connState = st; },
        onError: (msg) => { connError = msg; },
        // host=answerer：把解密后的明文帧交给 host 桥（demux → 本地执行 → 回结果）。
        onFrame: (plaintext) => bridge.handleFrame(plaintext),
      },
    );
    try {
      await provider.connect(s.deviceName);
      await notifyCloudActive(true);
    } catch (e) {
      connError = e instanceof Error ? e.message : tr('cloud.errConnectFailed');
    }
  }

  async function disconnect(): Promise<void> {
    provider?.disconnect();
    provider = null;
    hostBridge?.reset();
    hostBridge = null;
    connState = 'disconnected';
    await notifyCloudActive(false);
  }

  // 「最小化·后台保活」失败回调（契约 §8）：复用 enter_deep_root_mode，由
  // MinimizeButton 触发；前置校验失败（无活跃远控）等错误经此 inline 提示。
  function onMinimizeError(): void {
    connError = tr('cloud.errMinimizeFailed');
  }

  const isConnected = $derived(connState === 'connected');
  const isBusy = $derived(connState === 'connecting' || connState === 'handshaking');

  // §4：cloud 会话活跃（连接中/已连接）时拉取并轮询 TOTP（与 RemotePanel 同 5s 节奏，
  // 远小于 30s 周期，确保展示的 code 始终有效）；空闲时停轮询并清空展示。
  const sessionActive = $derived(isConnected || isBusy);
  $effect(() => {
    if (sessionActive) {
      void refreshRemoteInfo();
      totpTimer ??= setInterval(refreshRemoteInfo, 5000);
    } else {
      if (totpTimer) { clearInterval(totpTimer); totpTimer = null; }
      remoteInfo = null;
    }
    return () => { if (totpTimer) { clearInterval(totpTimer); totpTimer = null; } };
  });

  onMount(() => () => { if (totpTimer) clearInterval(totpTimer); });
</script>

<div class="space-y-4">
  <!-- 专属域名卡片 -->
  <div
    class="relative overflow-hidden rounded-xl border p-4"
    style="border-color: color-mix(in oklch, var(--rg-accent) 24%, var(--rg-border)); background: color-mix(in oklch, var(--rg-accent) 6%, var(--rg-surface));"
  >
    <div class="mb-2 flex items-center gap-2">
      <Globe class="h-4 w-4 text-[var(--rg-accent)]" />
      <span class="text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">{$t('cloud.publicEntry')}</span>
    </div>
    {#if domain}
      <code class="block break-all text-sm font-medium text-[var(--rg-fg)]">{domain}</code>
      <button
        onclick={openPublicRemote}
        class="mt-3 flex w-full items-center justify-center gap-2 rounded-lg border border-[var(--rg-accent)]/40 bg-[var(--rg-accent)]/10 py-2 text-sm font-semibold text-[var(--rg-accent)] transition-all hover:bg-[var(--rg-accent)]/20"
      >
        <ExternalLink class="h-4 w-4" /> {$t('cloud.openRemoteBtn')}
      </button>
      <p class="mt-1.5 text-[11px] text-[var(--rg-fg-muted)]">{$t('cloud.openRemoteHint')}</p>
    {:else}
      <p class="text-xs text-[var(--rg-fg-muted)]">{$t('cloud.entryPending')}</p>
    {/if}
  </div>

  {#if !hasDevice}
    <!-- 设备激活 -->
    <div class="rounded-xl border border-[var(--rg-border)] bg-[var(--rg-surface)]/50 p-4 space-y-3">
      <h3 class="text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">{$t('cloud.activateTitle')}</h3>
      <input
        bind:value={deviceNameInput}
        placeholder={$t('cloud.deviceNamePlaceholder')}
        class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 text-sm text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
      />
      <button
        onclick={activateDevice}
        disabled={activating || deviceNameInput.trim().length < 3}
        class="flex w-full items-center justify-center gap-2 rounded-lg bg-[var(--rg-accent)] py-2 text-sm font-semibold text-white transition-all hover:brightness-110 disabled:opacity-50"
      >
        {#if activating}<Loader2 class="h-4 w-4 animate-spin" />{:else}<Plus class="h-4 w-4" />{/if}
        {$t('cloud.activateBtn')}
      </button>
      {#if pairingHint}
        <p class="text-center text-[11px] text-[var(--rg-fg-muted)]">{pairingHint}</p>
      {/if}
    </div>
  {:else}
    <!-- 连接控制 + 状态 -->
    <div class="rounded-xl border border-[var(--rg-border)] bg-[var(--rg-surface)]/50 p-4 space-y-3">
      <div class="flex items-center justify-between">
        <span class="text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">{$t('cloud.connStatus')}</span>
        <span class="flex items-center gap-1.5 text-xs font-medium {isConnected ? 'text-green-400' : isBusy ? 'text-amber-400' : 'text-[var(--rg-fg-muted)]'}">
          {#if isConnected}<Wifi class="h-3.5 w-3.5" />{:else if isBusy}<Loader2 class="h-3.5 w-3.5 animate-spin" />{:else}<WifiOff class="h-3.5 w-3.5" />{/if}
          {stateLabel[connState]}
        </span>
      </div>

      {#if isConnected || isBusy}
        <button
          onclick={disconnect}
          class="flex w-full items-center justify-center gap-2 rounded-lg border border-[var(--rg-border)] py-2 text-sm font-medium text-[var(--rg-fg)] transition-colors hover:border-red-500/40 hover:text-red-400"
        >
          <Power class="h-4 w-4" /> {$t('cloud.disconnect')}
        </button>
      {:else}
        <button
          onclick={connect}
          class="flex w-full items-center justify-center gap-2 rounded-lg bg-[var(--rg-accent)] py-2 text-sm font-semibold text-white transition-all hover:brightness-110"
        >
          <Wifi class="h-4 w-4" /> {$t('cloud.connect')}
        </button>
      {/if}
    </div>

    <!-- §4 云端 TOTP 展示：会话活跃时显示本机 6 位 code + 绑定 QR（复用 LAN 布局），
         供用户读出输入到打开子域的浏览器 controller。 -->
    {#if sessionActive && remoteInfo}
      <div class="rounded-xl border border-[var(--rg-border)] bg-[var(--rg-surface)]/50 p-4 space-y-3">
        <h3 class="text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">{$t('cloud.totpTitle')}</h3>
        <p class="text-[11px] text-[var(--rg-fg-muted)]">{$t('cloud.totpHint')}</p>
        <div class="flex flex-col items-center gap-1 py-1">
          <p class="text-[10px] text-[var(--rg-fg-muted)] mb-1">{$t('remote.qrBindAuth')}</p>
          <QrCode value={remoteInfo.otpauthUri} size={140} />
        </div>
        <div class="flex items-center justify-between">
          <span class="text-xs text-[var(--rg-fg-muted)]">{$t('remote.totpCode')}</span>
          <span class="font-mono text-base font-bold tracking-wider text-[var(--rg-fg)]">{remoteInfo.totpCode}</span>
        </div>
      </div>
    {/if}

    <!-- 最小化·后台保活（契约 §8）：云端已连接时启用 -->
    <MinimizeButton active={isConnected} onError={onMinimizeError} />
  {/if}

  {#if connError}
    <p class="text-center text-xs text-red-400">{connError}</p>
  {/if}
</div>
