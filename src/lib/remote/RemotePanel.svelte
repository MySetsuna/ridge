<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import QrCode from './QrCode.svelte';
  import { Smartphone, RefreshCw, Power, PowerOff, Wifi, Zap, Globe, WifiOff, Loader2, Plus, ExternalLink, Monitor, Ban } from 'lucide-svelte';
  import { settingsStore, setSetting } from '$lib/stores/settings';
  import { refreshRemoteRunning, cloudHostOnline } from '$lib/stores/remoteStatus';
  import { t, tr } from '$lib/i18n';
  // §unify: 远程控制 = 同一份能力的两个触达通道(LAN + 官方公网)。本面板把二者
  // 合一为单一视图(去 tab):一个主开关、一份共享 TOTP、LAN/公网入口同屏、一份
  // 合并的「已连接」列表(来源用图标区分)、一个最小化按钮。公网通道由 premium 门控。
  import CloudProModal from './cloud/CloudProModal.svelte';
  import MinimizeButton from './MinimizeButton.svelte';
  import * as cloudAuth from './cloud/auth';
  import { cloudAuth as cloudAuthStore } from './cloud/auth';
  import { ApiError, listDevices, BASE_DOMAIN, type DeviceDto } from './cloud/apiClient';
  import { RidgeCloudHost, type CloudControllerSession, type HostSignalState } from './cloud/ridgeCloudProvider';
  import { CloudHostBridge } from './cloud/cloudHostBridge';
  import { makeCloudHostPaneSource } from './cloud/cloudHostPaneSource';

  let proModalOpen = $state(false);

  const cloudState = $derived($cloudAuthStore);
  // Premium 已就绪：已登录 + plan=premium（公网通道可用的前置）。
  const cloudReady = $derived(cloudAuth.isLoggedIn(cloudState) && cloudAuth.isPremium(cloudState));
  // 公网入口子域 + 是否已激活设备。
  const publicDomain = $derived(cloudAuth.publicEntryDomain(cloudState));
  const hasDevice = $derived(!!cloudState.deviceToken && !!cloudState.deviceName);
  // 用户名是激活设备/拼公网入口域名的前置。**入口在 ridge-cloud（网页账户页），
  // 桌面端只读取**：缺用户名时引导去网页设置，绝不在桌面端再提供输入。
  const hasUsername = $derived(!!cloudState.user?.username);

  // ── LAN（局域网/自建网）状态 ─────────────────────────────────────────────
  // Reflect the persisted/auto-restored state on mount (and stay in sync with
  // the Settings panel, which also reads `settingsStore.remoteEnabled`).
  const remoteEnabled = $derived($settingsStore.remoteEnabled);
  // §shared-TOTP: 单一来源 —— 同一本机 RemoteAuth(get_remote_info)，LAN 与公网共用。
  let remoteInfo = $state<{ port: number; lanIp: string; lanIps?: string[]; totpCode: string; otpauthUri: string; ready: boolean; machineName: string } | null>(null);
  // §lan-addresses: a phone may be on a different interface than the host's
  // primary route (Wi-Fi vs Tailscale vs Ethernet). List every usable LAN IPv4
  // and let the user pick the one on their phone's network — the QR + copy link
  // follow the selection. Defaults to the primary (route-to-internet) address.
  let selectedIp = $state<string | null>(null);
  const lanIps = $derived(
    remoteInfo?.lanIps && remoteInfo.lanIps.length > 0
      ? remoteInfo.lanIps
      : remoteInfo?.lanIp
        ? [remoteInfo.lanIp]
        : [],
  );
  const activeIp = $derived(
    selectedIp && lanIps.includes(selectedIp)
      ? selectedIp
      : (remoteInfo?.lanIp ?? lanIps[0] ?? 'localhost'),
  );
  let connectError = $state('');
  let totpTimer: ReturnType<typeof setInterval> | null = null;
  let machineName = $state('Ridge');
  let copySuccess = $state(false);

  // §sessions: connected LAN remote-control sessions, fetched via Tauri (the
  // desktop has direct AppState access — no need to connect as a WS client).
  interface SessionDto { id: number; remoteAddr: string; deviceId: string; userAgent: string; connectedSecs: number; }
  let sessions = $state<SessionDto[]>([]);
  let sessionsTimer: ReturnType<typeof setInterval> | null = null;

  // §blacklist: persistent device/IP bans (snake_case keys from the Rust struct).
  interface BlacklistDto { id: string; device_id?: string | null; ip?: string | null; label: string; added_at: number; }
  let blacklist = $state<BlacklistDto[]>([]);

  // ── 公网（官方公网加速，契约 §5.3 多控制方）状态 ─────────────────────────
  // host 多控制方管理器（一条信令 WS + 按 cid 的 N 个 PeerConnection）。
  let host: RidgeCloudHost | null = null;
  // 零信任 #2（概念 4-桌面）：本机 Ed25519 设备身份公钥（get_device_identity_pub 取一次缓存）。
  // 与 sign_device_identity 配对注入 host：俱在 → 握手发 0x02 签名帧；取不到 → 回落 0x01。
  let deviceIdentityPub: Uint8Array | null = null;
  let hostState = $state<HostSignalState>('offline');
  let cloudSessions = $state<CloudControllerSession[]>([]);
  const isOnline = $derived(hostState === 'online');
  const isConnecting = $derived(hostState === 'connecting');
  // 「是否有人在使用」公网：已完成 E2EE 握手、可真正操作的控制方。
  const activeCount = $derived(cloudSessions.filter((s) => s.state === 'connected').length);
  // 云端已注册设备（GET /devices）：本账户名下设备及在线状态。
  let devices = $state<DeviceDto[]>([]);
  let devicesTimer: ReturnType<typeof setInterval> | null = null;
  // 设备激活（只需设备名；用户名取自登录态）。
  let activating = $state(false);
  let deviceNameInput = $state('');
  let pairingHint = $state('');

  const stateLabel = $derived<Record<CloudControllerSession['state'], string>>({
    disconnected: $t('cloud.stateDisconnected'),
    connecting: $t('cloud.stateConnecting'),
    handshaking: $t('cloud.stateHandshaking'),
    connected: $t('cloud.stateConnected'),
    error: $t('cloud.stateError'),
  });

  // ── 合并的「已连接」列表（LAN sessions + cloud controllers，来源用图标区分）──
  interface ConnRow {
    source: 'lan' | 'cloud';
    key: string;
    title: string;
    subtitle: string;
    connected: boolean;
    onDisconnect: () => void;
    onBlock: () => void;
  }
  const connectedClients = $derived<ConnRow[]>([
    ...sessions.map((s): ConnRow => ({
      source: 'lan',
      key: `lan-${s.id}`,
      title: deviceLabel(s),
      subtitle: tr('remote.connectedFor', { addr: s.remoteAddr, min: Math.floor(s.connectedSecs / 60) }),
      connected: true,
      onDisconnect: () => disconnectSession(s.id),
      onBlock: () => blacklistSession(s.id),
    })),
    ...cloudSessions.map((c): ConnRow => ({
      source: 'cloud',
      key: `cloud-${c.cid}`,
      title: tr('cloud.controllerName', { id: c.cid }),
      subtitle: `${stateLabel[c.state]} · ${tr('cloud.connectedFor', { min: sinceMinutes(c.connectedAt) })}`,
      connected: c.state === 'connected',
      onDisconnect: () => disconnectController(c.cid),
      onBlock: () => blacklistController(c.cid),
    })),
  ]);

  // ── LAN actions ──────────────────────────────────────────────────────────
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
    return s.remoteAddr || tr('remote.unknownDevice');
  }
  async function refreshSessions() {
    try { sessions = await invoke<SessionDto[]>('list_remote_sessions'); } catch { sessions = []; }
  }
  async function disconnectSession(id: number) {
    try { await invoke('disconnect_session', { id }); } catch { /* ignore */ }
    refreshSessions();
  }
  function buildLinkUri(lanIp: string, port: number): string {
    // Always point the phone at THIS instance's own remote server over HTTPS
    // (self-signed → secure context). It serves the built `static/remote` bundle
    // and handles /verify, /ws same-origin.
    //
    // §dev: deliberately NOT the Vite dev server (`:5174`). That path proxies
    // /verify,/ws to a hardcoded `http://127.0.0.1:9527`, which breaks whenever
    // the server is on TLS or a non-9527 port (e.g. two instances colliding on
    // the default port → the dev server lands on 9528). Pointing the QR straight
    // at the running instance's actual `port` makes "scan → connect" work against
    // the dev instance with no proxy in the path. Trade-off: no phone-side HMR —
    // rebuild with `pnpm build:remote` to refresh the served bundle.
    return `https://${lanIp}:${port}/`;
  }
  async function refreshRemoteInfo() {
    try {
      const info = await invoke<{ port: number; lanIp: string; lanIps?: string[]; totpCode: string; otpauthUri: string; ready: boolean; machineName: string }>('get_remote_info');
      remoteInfo = info;
      machineName = info.machineName;
    } catch (e: unknown) {
      console.error('Failed to refresh remote info', e);
    }
  }

  // §totp-persist：重置本机 TOTP 种子（桌面 host 专属；web-remote 不渲染该按钮）。
  // 二次确认后调命令；Rust 发 remote-totp-changed → onMount 的 listener 刷新二维码。
  let resettingTotp = $state(false);
  async function resetTotp(): Promise<void> {
    if (resettingTotp) return;
    const { confirm } = await import('@tauri-apps/plugin-dialog');
    const ok = await confirm($t('remote.resetTotpConfirm'), { title: $t('remote.resetTotp'), kind: 'warning' });
    if (!ok) return;
    resettingTotp = true;
    try {
      await invoke('remote_reset_totp');
      await refreshRemoteInfo();
    } catch (e: unknown) {
      connectError = e instanceof Error ? e.message : tr('remote.toggleFailed');
    } finally {
      resettingTotp = false;
    }
  }
  async function toggleRemoteEnabled() {
    try {
      const newState = !remoteEnabled;
      await invoke('set_remote_enabled', { enabled: newState });
      setSetting('remoteEnabled', newState);
      await refreshRemoteRunning();
      if (newState) { await refreshRemoteInfo(); await refreshSessions(); }
    } catch (e: unknown) {
      connectError = e instanceof Error ? e.message : tr('remote.toggleFailed');
      void refreshRemoteRunning();
    }
  }
  async function copyLink() {
    const uri = buildLinkUri(activeIp, remoteInfo?.port ?? 0);
    try {
      await navigator.clipboard.writeText(uri);
      copySuccess = true;
      setTimeout(() => copySuccess = false, 2000);
    } catch { /* clipboard not available */ }
  }

  // ── 公网 actions（移植自原 CloudPanel，逻辑不变）──────────────────────────
  function codeToMessage(code: string): string {
    const msg = tr(`errors.${code}`);
    return msg === `errors.${code}` ? tr('errors.GENERIC') : msg;
  }
  function sinceMinutes(connectedAt: number): number {
    return Math.max(0, Math.floor((Date.now() - connectedAt) / 60000));
  }
  // 跨 agent 命令：通知 Rust 侧云端远控活跃状态（契约 §8.1）。容错。
  async function notifyCloudActive(active: boolean): Promise<void> {
    try { await invoke('set_cloud_remote_active', { active }); } catch { /* 容错 */ }
  }
  async function refreshDevices(): Promise<void> {
    const s = cloudAuth.snapshot();
    if (!s.userToken) { devices = []; return; }
    try { const res = await listDevices(s.userToken); devices = res.devices ?? []; } catch { /* 保留上次列表 */ }
  }
  // opener 优先在默认浏览器打开外链；不可用（纯浏览器/测试）时退回 window.open。
  // 公网远控入口、设备子域、账户页等多处共用，集中一处避免重复。
  async function openExternal(url: string): Promise<void> {
    try {
      const opener = await import('@tauri-apps/plugin-opener');
      await opener.openUrl(url);
    } catch {
      try { window.open(url, '_blank', 'noopener'); } catch { /* 静默 */ }
    }
  }
  // 在默认浏览器打开本机专属子域（controller 入口，契约 §3 流程第 5 步）。
  async function openPublicRemote(): Promise<void> {
    if (!publicDomain) return;
    await openExternal(`https://${publicDomain}`);
  }
  // 某云端设备的公网远控入口子域（契约 §1：{device}-{username}.{base}）。
  // 用户名取自登录态（桌面端只读取，不提供设置）；缺用户名无法成域时返回 null。
  function deviceRemoteUrl(name: string): string | null {
    const username = cloudState.user?.username;
    if (!username) return null;
    return `https://${name}-${username}.${BASE_DOMAIN}`;
  }
  // 点击云端设备：在默认浏览器打开其公网远控连接。
  async function openDeviceRemote(name: string): Promise<void> {
    const url = deviceRemoteUrl(name);
    if (url) await openExternal(url);
  }
  // §username: 用户名只能在 ridge-cloud 网页账户页设置（设置一次、不可改），桌面端
  // 只读取。缺用户名时在浏览器打开账户页引导设置，回来后「刷新」重拉 /me 同步。
  let refreshingUser = $state(false);
  async function openCloudAccount(): Promise<void> {
    await openExternal(`https://${BASE_DOMAIN}/`);
  }
  async function refreshCloudUser(): Promise<void> {
    if (refreshingUser) return;
    connectError = '';
    refreshingUser = true;
    try {
      await cloudAuth.refreshMe();
    } catch (e) {
      connectError = e instanceof ApiError ? codeToMessage(e.code) : tr('cloud.errGeneric');
    } finally {
      refreshingUser = false;
    }
  }
  async function activateDevice(): Promise<void> {
    connectError = '';
    activating = true;
    pairingHint = '';
    try {
      await cloudAuth.activateThisDevice(deviceNameInput.trim(), (p) => {
        pairingHint = tr('cloud.pairingHint', { code: p.pairingCode, sec: p.expiresIn });
      });
      pairingHint = '';
    } catch (e) {
      connectError = e instanceof ApiError ? codeToMessage(e.code) : tr('cloud.errActivateFailed');
    } finally {
      activating = false;
    }
  }
  /** 构造 host 管理器：每个 controller 一个独立 CloudHostBridge（pane 输出各自订阅）。 */
  function buildHost(): RidgeCloudHost | null {
    const s = cloudAuth.snapshot();
    if (!s.deviceToken || !s.deviceName || !s.user?.username) return null;
    return new RidgeCloudHost(
      {
        deviceToken: s.deviceToken,
        username: s.user.username,
        // 零信任 #2（概念 4-桌面）：host 握手发 0x02 设备签名帧。signContext = 对 id-bind
        // context 做 Ed25519 签名（私钥在 Rust/DPAPI，relay 无法伪造）；identityPub = 本机
        // 设备身份公钥（启动取一次缓存）。两者配对：俱在 → 0x02；缺一 → 回落 0x01（向后兼容）。
        signContext: (context: Uint8Array) =>
          invoke<number[]>('sign_device_identity', { context: Array.from(context) }).then((a) =>
            Uint8Array.from(a),
          ),
        identityPub: deviceIdentityPub ?? undefined,
      },
      {
        onHostState: (st) => {
          hostState = st;
          if (st === 'error') connectError = tr('cloud.hostError');
          if (st === 'online' || st === 'connecting') connectError = '';
          // Surface "public remote is serving" to the whole app so per-pane
          // refresh buttons (RidgePane) appear while a cloud viewer can share
          // the PTY — the LAN-only `remoteRunning` store stays false here.
          cloudHostOnline.set(st === 'online');
        },
        onSessions: (list) => { cloudSessions = list; },
        onError: (msg) => { connectError = msg; },
        // host=Tauri 桌面 app：注入真实 invoke + pane 源 + 本机 TOTP 校验（契约 §0/§4/§5.1）。
        createBridge: (_cid, send, bindTranscript) =>
          new CloudHostBridge({
            invoke: (method, params) => invoke(method, params),
            sendFrame: send,
            // B2（D-GM-11）：用 subscribe_pane_raw 专用 raw fan-out（RemotePtyEvent::
            // RawBytes → Tauri event pane-raw-{pane}）。**必须**走此路而非订阅
            // pty-output：原生桌面 pane 为 delta-mode，lib.rs 对其 `continue` 跳过了
            // pty-output 发射（只发 pty-delta），故旧 cloudPaneSource 对原生 pane 收不到
            // 字节。raw fan-out 在 delta 分支之前，delta-mode 也照样推。
            paneOutputSource: makeCloudHostPaneSource({ invoke, listen }),
            // 明文 totp-verify（旧 controller / host 回落 0x01 时）。
            totpVerifier: (code) => invoke<boolean>('verify_remote_totp', { code }),
            // 零信任 #1（概念 5）：host 发 0x02 → bindTranscript 非空时启用 totp-bind
            // 信道绑定校验（HMAC tag，明文码不上线）。transcript 闭包注入 Rust 命令。
            totpBindVerifier: bindTranscript
              ? (tag) =>
                  invoke<boolean>('verify_remote_totp_bind', {
                    transcript: Array.from(bindTranscript),
                    tag: Array.from(tag),
                  })
              : undefined,
          }),
      },
    );
  }
  async function goOnline(): Promise<void> {
    connectError = '';
    const s = cloudAuth.snapshot();
    if (!s.deviceToken || !s.deviceName || !s.user?.username) {
      connectError = tr('cloud.errDeviceNotActivated');
      return;
    }
    // 零信任 #2（概念 4-桌面）：取一次本机设备身份公钥缓存，供 host 握手发 0x02。
    // 取不到（旧设备/无密钥）→ 留 null，host 自动回落 0x01（不阻断上线）。
    if (!deviceIdentityPub) {
      try {
        deviceIdentityPub = Uint8Array.from(await invoke<number[]>('get_device_identity_pub'));
      } catch {
        deviceIdentityPub = null;
      }
    }
    host ??= buildHost();
    if (!host) { connectError = tr('cloud.errDeviceNotActivated'); return; }
    try {
      await host.goOnline(s.deviceName);
      await notifyCloudActive(true);
    } catch (e) {
      connectError = e instanceof Error ? e.message : tr('cloud.errConnectFailed');
    }
  }
  async function goOffline(): Promise<void> {
    host?.goOffline();
    cloudHostOnline.set(false);
    await notifyCloudActive(false);
  }
  function disconnectController(cid: string): void { host?.kick(cid); }
  function blacklistController(cid: string): void { host?.blacklist(cid); }

  // 切到公网相关操作前的门控：未就绪(未登录/未订阅)弹 Pro Modal。
  function requirePremium(): boolean {
    if (!cloudReady) { proModalOpen = true; return false; }
    return true;
  }
  function onCloudReady(): void { /* 登录/激活成功：cloudReady 派生态自动更新 UI */ }

  // ── polling ──────────────────────────────────────────────────────────────
  // §sessions: poll the connected LAN sessions while remote control is enabled.
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

  // 已激活设备：进面板即拉一次云端设备列表，并低频轮询在线状态。
  $effect(() => {
    if (hasDevice) {
      void refreshDevices();
      devicesTimer ??= setInterval(refreshDevices, 10000);
    } else {
      if (devicesTimer) { clearInterval(devicesTimer); devicesTimer = null; }
      devices = [];
    }
    return () => { if (devicesTimer) { clearInterval(devicesTimer); devicesTimer = null; } };
  });

  onMount(() => {
    refreshRemoteInfo();
    // §totp-persist：种子被重置 / 登录态切换后，Rust 发此事件 → 刷新二维码+码。
    const unlistenTotp = listen('remote-totp-changed', () => { void refreshRemoteInfo(); });
    // §shared-TOTP: single 5s poll for the shared TOTP code, alive while either
    // channel is active (LAN enabled OR public host online/connecting).
    totpTimer = setInterval(async () => {
      if (remoteEnabled || isOnline || isConnecting) await refreshRemoteInfo();
    }, 5000);
    return () => {
      if (totpTimer) clearInterval(totpTimer);
      if (devicesTimer) clearInterval(devicesTimer);
      host?.goOffline();
      void unlistenTotp.then((un) => un());
    };
  });
</script>

<div class="flex flex-col h-full">
  <!-- Header -->
  <div class="flex items-center justify-between px-3 h-10 border-b border-[var(--rg-border)] shrink-0">
    <h2 class="text-xs font-semibold text-[var(--rg-fg)] uppercase tracking-wider flex items-center gap-1.5">
      <Smartphone class="w-3.5 h-3.5" />
      {$t('remote.title', { name: machineName })}
    </h2>
  </div>

  <div class="flex-1 overflow-auto p-3 space-y-4">
    <!-- ① 主开关：启用远程控制（LAN 基础通道） -->
    <div class="flex flex-col items-center gap-2 pt-1">
      <button
        onclick={toggleRemoteEnabled}
        class="w-full h-10 rounded-lg font-medium text-sm flex items-center justify-center gap-2 transition-all duration-200 {remoteEnabled
          ? 'bg-green-500/15 text-green-400 border border-green-500/30 hover:bg-green-500/25'
          : 'bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] border border-[var(--rg-border)] hover:border-[var(--rg-accent)]/30 hover:text-[var(--rg-fg)]'}"
      >
        {#if remoteEnabled}
          <Power class="w-4 h-4" />
          {$t('remote.enabledLabel')}
          <span class="w-2 h-2 rounded-full bg-green-400 animate-pulse"></span>
        {:else}
          <PowerOff class="w-4 h-4" />
          {$t('remote.startLabel')}
        {/if}
      </button>
    </div>

    <!-- ② 共享 TOTP（LAN 与公网同一本机 RemoteAuth）：任一通道活跃即展示。
         按 remoteInfo 存在性(非 .ready)判断 —— totpCode 来自本机 RemoteAuth,与
         LAN remote_enabled 无关;公网-only(未开 LAN)时 controller 同样需要它。 -->
    {#if (remoteEnabled || isOnline) && remoteInfo}
      <div class="bg-[var(--rg-surface)]/50 rounded-lg p-3 space-y-2">
        <div class="flex items-center justify-between">
          <span class="text-[10px] font-semibold text-[var(--rg-fg-muted)] uppercase tracking-wider">{$t('remote.totpCode')}</span>
          <span class="text-[var(--rg-fg)] font-mono font-bold tracking-wider text-base">{remoteInfo.totpCode}</span>
        </div>
        <div class="flex flex-col items-center gap-1 pt-1">
          <p class="text-[10px] text-[var(--rg-fg-muted)]">{$t('remote.qrBindAuth')}</p>
          {#if import.meta.env.RIDGE_WEB_REMOTE !== true}
            <button
              onclick={resetTotp}
              disabled={resettingTotp}
              class="flex items-center gap-1 text-[10px] text-[var(--rg-fg-muted)] hover:text-red-400 transition-colors disabled:opacity-50"
              title={$t('remote.resetTotp')}
            >
              <RefreshCw class="w-3 h-3 {resettingTotp ? 'animate-spin' : ''}" />
              {$t('remote.resetTotp')}
            </button>
          {/if}
          <QrCode value={remoteInfo.otpauthUri} size={132} />
        </div>
      </div>
    {/if}

    <!-- ③ 入口区：LAN 卡 + 公网卡（同屏并列） -->
    <div class="grid grid-cols-1 gap-3">
      <!-- LAN 入口卡 -->
      <div class="rounded-xl border border-[var(--rg-border)] bg-[var(--rg-surface)]/50 p-3 space-y-2">
        <div class="flex items-center gap-1.5">
          <Wifi class="h-3.5 w-3.5 text-[var(--rg-fg-muted)]" />
          <span class="text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">{$t('remote.modeLan')}</span>
        </div>
        {#if remoteEnabled}
          {#if remoteInfo?.ready}
            <div class="flex flex-col items-center gap-1 py-1">
              <QrCode value={buildLinkUri(activeIp, remoteInfo.port)} size={132} />
              <p class="text-[9px] text-[var(--rg-fg-muted)]">{$t('remote.qrScanFlow')}</p>
              <!-- Self-signed HTTPS in both dev and prod now → always surface the
                   trust-the-cert hint. -->
              <p class="text-[9px] text-amber-400/80 text-center leading-snug max-w-[180px]">{$t('remote.certWarn')}</p>
            </div>
            {#if lanIps.length > 1}
              <!-- §lan-addresses: pick the address on the phone's network -->
              <div class="flex flex-wrap gap-1 justify-center pt-0.5">
                {#each lanIps as ip (ip)}
                  <button
                    onclick={() => selectedIp = ip}
                    class="px-2 py-0.5 rounded text-[10px] font-mono border transition-colors {ip === activeIp
                      ? 'border-[var(--rg-accent)] text-[var(--rg-accent)] bg-[var(--rg-accent)]/10'
                      : 'border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:border-[var(--rg-accent)]/40'}"
                    title={ip}
                  >
                    {ip}
                  </button>
                {/each}
              </div>
              <p class="text-[9px] text-[var(--rg-fg-muted)] text-center leading-snug">{$t('remote.lanPickAddress')}</p>
            {/if}
            <div class="flex items-center justify-between text-xs">
              <span class="text-[var(--rg-fg-muted)]">{$t('remote.mobileEntry')}</span>
              <button onclick={copyLink} class="text-[var(--rg-accent)] font-mono hover:underline cursor-pointer bg-transparent border-none p-0" title={$t('remote.copyLinkTitle')}>
                {activeIp}:{remoteInfo.port}
              </button>
            </div>
            <button onclick={copyLink} class="w-full text-[10px] text-[var(--rg-accent)] hover:underline" title={$t('remote.copyLink')}>
              {copySuccess ? $t('remote.linkCopied') : $t('remote.copyLink')}
            </button>
          {:else}
            <div class="flex items-center gap-2 py-2 text-[var(--rg-fg-muted)]">
              <RefreshCw class="w-4 h-4 animate-spin text-[var(--rg-accent)]" />
              <span class="text-xs">{$t('remote.fetchingInfo')}</span>
            </div>
          {/if}
        {:else}
          <p class="text-[11px] text-[var(--rg-fg-muted)]">{$t('remote.startLabel')}</p>
        {/if}
      </div>

      <!-- 公网入口卡 -->
      <div
        class="relative overflow-hidden rounded-xl border p-3 space-y-2"
        style="border-color: color-mix(in oklch, var(--rg-accent) 24%, var(--rg-border)); background: color-mix(in oklch, var(--rg-accent) 6%, var(--rg-surface));"
      >
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-1.5">
            <Globe class="h-3.5 w-3.5 text-[var(--rg-accent)]" />
            <span class="text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">{$t('remote.modeCloud')}</span>
            {#if !cloudReady}<span class="rounded bg-[var(--rg-accent)]/20 px-1 text-[9px] text-[var(--rg-accent)]">Pro</span>{/if}
          </div>
          {#if hasDevice}
            <span class="flex items-center gap-1.5 text-[11px] font-medium {isOnline ? 'text-green-400' : isConnecting ? 'text-amber-400' : hostState === 'error' ? 'text-red-400' : 'text-[var(--rg-fg-muted)]'}">
              {#if isOnline}<Wifi class="h-3.5 w-3.5" />{:else if isConnecting}<Loader2 class="h-3.5 w-3.5 animate-spin" />{:else}<WifiOff class="h-3.5 w-3.5" />{/if}
              {isOnline ? $t('cloud.hostOnline') : isConnecting ? $t('cloud.hostConnecting') : hostState === 'error' ? $t('cloud.stateError') : $t('cloud.hostOffline')}
            </span>
          {/if}
        </div>

        {#if !cloudReady}
          <!-- 未就绪：引导升级 / 登录 -->
          <p class="text-[11px] text-[var(--rg-fg-muted)]">{$t('cloud.entryPending')}</p>
          <button
            onclick={() => requirePremium()}
            class="flex w-full items-center justify-center gap-2 rounded-lg bg-[var(--rg-accent)] py-2 text-sm font-semibold text-white transition-all hover:brightness-110"
          >
            <Zap class="h-4 w-4" /> {$t('cloud.enablePublic')}
          </button>
        {:else if !hasUsername}
          <!-- 已就绪但未设用户名：入口在 ridge-cloud（网页账户页），桌面端只引导、不提供输入 -->
          <p class="text-[11px] leading-relaxed text-[var(--rg-fg-muted)]">{$t('cloud.usernameRequiredHint')}</p>
          <button
            onclick={openCloudAccount}
            class="flex w-full items-center justify-center gap-2 rounded-lg bg-[var(--rg-accent)] py-2 text-sm font-semibold text-white transition-all hover:brightness-110"
          >
            <ExternalLink class="h-4 w-4" /> {$t('cloud.goSetUsername')}
          </button>
          <button
            onclick={refreshCloudUser}
            disabled={refreshingUser}
            class="flex w-full items-center justify-center gap-2 rounded-lg border border-[var(--rg-border)] py-2 text-xs font-medium text-[var(--rg-fg)] transition-colors hover:border-[var(--rg-accent)]/40 hover:bg-white/5 disabled:opacity-50"
          >
            {#if refreshingUser}<Loader2 class="h-3.5 w-3.5 animate-spin" />{:else}<RefreshCw class="h-3.5 w-3.5" />{/if}
            {$t('cloud.refreshAfterSet')}
          </button>
        {:else if !hasDevice}
          <!-- 已就绪但未激活设备：输设备名激活 -->
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
          {#if pairingHint}<p class="text-center text-[11px] text-[var(--rg-fg-muted)]">{pairingHint}</p>{/if}
        {:else}
          <!-- 已激活：域名 + 打开 + 启用/停用公网 -->
          {#if publicDomain}
            <code class="block break-all text-xs font-medium text-[var(--rg-fg)]">{publicDomain}</code>
          {/if}
          {#if isOnline}
            <p class="text-[11px] {activeCount > 0 ? 'text-green-400' : 'text-[var(--rg-fg-muted)]'}">
              {activeCount > 0 ? $t('cloud.inUse', { count: activeCount }) : $t('cloud.idle')}
            </p>
            <button
              onclick={openPublicRemote}
              class="flex w-full items-center justify-center gap-2 rounded-lg border border-[var(--rg-accent)]/40 bg-[var(--rg-accent)]/10 py-2 text-sm font-semibold text-[var(--rg-accent)] transition-all hover:bg-[var(--rg-accent)]/20"
            >
              <ExternalLink class="h-4 w-4" /> {$t('cloud.openRemoteBtn')}
            </button>
            {#if publicDomain}
              <div class="flex flex-col items-center gap-1 py-1">
                <p class="text-[10px] text-[var(--rg-fg-muted)]">{$t('cloud.qrOpenRemote')}</p>
                <QrCode value={`https://${publicDomain}`} size={132} />
              </div>
            {/if}
            <button
              onclick={goOffline}
              class="flex w-full items-center justify-center gap-2 rounded-lg border border-[var(--rg-border)] py-2 text-sm font-medium text-[var(--rg-fg)] transition-colors hover:border-red-500/40 hover:text-red-400"
            >
              <Power class="h-4 w-4" /> {$t('cloud.disablePublic')}
            </button>
          {:else}
            <button
              onclick={goOnline}
              disabled={isConnecting}
              class="flex w-full items-center justify-center gap-2 rounded-lg bg-[var(--rg-accent)] py-2 text-sm font-semibold text-white transition-all hover:brightness-110 disabled:opacity-50"
            >
              {#if isConnecting}<Loader2 class="h-4 w-4 animate-spin" />{:else}<Wifi class="h-4 w-4" />{/if}
              {$t('cloud.enablePublic')}
            </button>
          {/if}
        {/if}
      </div>
    </div>

    <!-- ④ 合并的「已连接」列表（LAN + 公网，图标区分来源） -->
    {#if remoteEnabled || isOnline}
      <div class="bg-[var(--rg-surface)]/50 rounded-lg p-3 space-y-2">
        <h3 class="text-[10px] font-semibold text-[var(--rg-fg-muted)] uppercase tracking-wider">
          {$t('remote.connectedDevices', { count: connectedClients.length })}
        </h3>
        {#each connectedClients as c (c.key)}
          <div class="flex items-center justify-between py-1.5 px-2 rounded-md hover:bg-[var(--rg-surface)] transition-colors">
            <div class="min-w-0 flex-1">
              <p class="text-xs text-[var(--rg-fg)] truncate flex items-center gap-1.5" title={c.title}>
                {#if c.source === 'lan'}<Wifi class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)]" />{:else}<Globe class="h-3 w-3 shrink-0 text-[var(--rg-accent)]" />{/if}
                {c.title}
              </p>
              <p class="text-[10px] {c.connected ? 'text-[var(--rg-fg-muted)]' : 'text-amber-400'} truncate">{c.subtitle}</p>
            </div>
            <div class="shrink-0 ml-2 flex items-center gap-1">
              <button
                onclick={c.onDisconnect}
                class="px-2 py-1 rounded text-[10px] font-medium border border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors"
                title={$t('remote.disconnectTitle')}
              >
                {$t('remote.disconnectBtn')}
              </button>
              <button
                onclick={c.onBlock}
                class="px-2 py-1 rounded text-[10px] font-medium border border-red-500/30 text-red-400 hover:bg-red-500/10 transition-colors flex items-center gap-1"
                title={$t('remote.blockTitle')}
              >
                <Ban class="h-3 w-3" /> {$t('remote.blockBtn')}
              </button>
            </div>
          </div>
        {/each}
        {#if connectedClients.length === 0}
          <p class="text-[11px] text-[var(--rg-fg-muted)] py-1">{$t('remote.noConnections')}</p>
        {/if}
      </div>
    {/if}

    <!-- ⑤ 黑名单（LAN 持久封禁） -->
    {#if blacklist.length > 0}
      <div class="bg-[var(--rg-surface)]/50 rounded-lg p-3 space-y-2">
        <h3 class="text-[10px] font-semibold text-[var(--rg-fg-muted)] uppercase tracking-wider">
          {$t('remote.blacklist', { count: blacklist.length })}
        </h3>
        {#each blacklist as b (b.id)}
          <div class="flex items-center justify-between py-1.5 px-2 rounded-md hover:bg-[var(--rg-surface)] transition-colors">
            <div class="min-w-0 flex-1">
              <p class="text-xs text-[var(--rg-fg)] truncate">{b.label}</p>
              <p class="text-[10px] text-[var(--rg-fg-muted)] truncate">
                {b.device_id ? $t('remote.blacklistDevice', { id: b.device_id.slice(0, 8) }) : ''}{b.device_id && b.ip ? ' · ' : ''}{b.ip ?? ''}
              </p>
            </div>
            <button
              onclick={() => unblacklist(b.id)}
              class="shrink-0 ml-2 px-2 py-1 rounded text-[10px] font-medium border border-[var(--rg-border)] text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/10 transition-colors"
              title={$t('remote.unblockTitle')}
            >
              {$t('remote.unblockBtn')}
            </button>
          </div>
        {/each}
      </div>
    {/if}

    <!-- ⑥ 云端已注册设备（GET /devices）：本账户名下设备及在线状态。
         高亮「本机」（cloudState.deviceName）；点击任一设备在默认浏览器打开其公网远控。 -->
    {#if devices.length > 0}
      <div class="bg-[var(--rg-surface)]/50 rounded-lg p-3 space-y-2">
        <h3 class="text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">{$t('cloud.cloudDevicesTitle')}</h3>
        {#each devices as d (d.name)}
          {@const isThisMachine = d.name === cloudState.deviceName}
          <button
            type="button"
            onclick={() => openDeviceRemote(d.name)}
            disabled={!hasUsername}
            title={$t('cloud.openDeviceRemoteTitle')}
            class="group flex w-full items-center justify-between gap-2 rounded-md py-1.5 px-2 text-left transition-colors disabled:cursor-default disabled:opacity-60
              {isThisMachine
                ? 'border border-[var(--rg-accent)]/40 bg-[var(--rg-accent)]/10'
                : 'border border-transparent hover:bg-[var(--rg-surface)]'}"
          >
            <span class="min-w-0 flex items-center gap-1.5">
              <Monitor class="h-3.5 w-3.5 shrink-0 {isThisMachine ? 'text-[var(--rg-accent)]' : 'text-[var(--rg-fg-muted)]'}" />
              <span class="truncate text-xs text-[var(--rg-fg)] {isThisMachine ? 'font-medium' : ''}">{d.name}</span>
              {#if isThisMachine}
                <span class="shrink-0 rounded bg-[var(--rg-accent)]/20 px-1 text-[9px] font-medium text-[var(--rg-accent)]">{$t('cloud.thisMachine')}</span>
              {/if}
              {#if hasUsername}
                <ExternalLink class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)] opacity-0 transition-opacity group-hover:opacity-100" />
              {/if}
            </span>
            <span class="shrink-0 text-[10px] flex items-center gap-1 {d.online ? 'text-green-400' : 'text-[var(--rg-fg-muted)]'}">
              <span class="w-1.5 h-1.5 rounded-full {d.online ? 'bg-green-400' : 'bg-[var(--rg-fg-muted)]'}"></span>
              {d.online ? $t('cloud.deviceOnline') : $t('cloud.deviceOffline')}
            </span>
          </button>
        {/each}
      </div>
    {/if}

    <!-- ⑦ 最小化·后台保活（契约 §8）：任一通道活跃时启用 -->
    {#if remoteEnabled || isOnline}
      <MinimizeButton active={remoteEnabled || isOnline} onError={(m) => (connectError = m || tr('cloud.errMinimizeFailed'))} />
    {/if}

    {#if connectError}
      <p class="text-xs text-red-400 text-center">{connectError}</p>
    {/if}
  </div>
</div>

<!-- §cloud: Pro 升级 / 登录 Modal（公网未就绪时引导）-->
<CloudProModal bind:open={proModalOpen} onClose={() => (proModalOpen = false)} onReady={onCloudReady} />
