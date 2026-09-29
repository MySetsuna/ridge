<script lang="ts">
  // §perf 按 mode 懒加载：AuthScreen / CloudAuthScreen / MainApp 原随顶层 import
  // 全进 eager graph。改为 mode 决定哪个 auth screen 动态 import，verified 后才 load
  // MainApp。LAN 用户不下载 cloud 门，cloud 用户不下载 LAN 门。
  import { RemoteConnection, createLanWsTransport, type RemoteLink } from '@ridge/remote';
  import { setTransport } from '$lib/transport';
  import { WsDataProvider } from '$lib/transport/ws';
  import { bridge } from '$lib/transport/tauriShim/bridge';
  import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query';

  // §mobile-cloud (design 2026-06-16): the mobile app now has TWO transports —
  //   - LAN:   RemoteConnection (WebSocket, self-signed TLS) — phone on same network.
  //   - CLOUD: CloudRemoteConnection (WebRTC E2EE + zero-trust) — public tenant subdomain.
  // The relay serves this bundle to mobile UAs only on a tenant subdomain
  // ({device}-{username}.{base}); the LAN host serves it on its own IP/.local name.
  // So the hostname tells us which transport to boot. The strict §1.1 tenant parse
  // (and its login redirect) runs inside CloudAuthScreen; here we only need a cheap
  // route so the LAN path never imports the heavy cloud/WebRTC/E2EE bundle.
  function looksLikeCloudHost(): boolean {
    if (typeof location === 'undefined') return false;
    try {
      if (new URLSearchParams(location.search).has('cloudHost')) return true;
    } catch { /* malformed search — fall through to hostname */ }
    const host = location.hostname;
    if (!host || host === 'localhost') return false;
    // Tenant cloud entry is `{device}-{username}.{base}` and the base is itself a
    // multi-label public domain (e.g. 9527127.xyz), so a tenant host has ≥3 labels
    // with a hyphen in the FIRST one. That single test rules out LAN access cleanly:
    //   - IPv4 (192.168.1.5): first label "192" has no hyphen → false
    //   - mDNS / single-dot LAN (host.local, host.lan): only 2 labels → false
    //   - bare machine name (jacks-laptop): only 1 label → false
    // A residual misroute is still caught in CloudAuthScreen (strict §1.1 parse →
    // onfallbacklan).
    const labels = host.split('.');
    return labels.length >= 3 && labels[0].includes('-');
  }

  // Resolved synchronously — this is a pure client SPA, `location` is always present.
  // Compute the initial socket as a plain const so the $state inits don't reference
  // one another (avoids Svelte's state_referenced_locally warning).
  const initialCloud = looksLikeCloudHost();
  const initialLan = initialCloud ? null : new RemoteConnection();
  let mode = $state<'cloud' | 'lan'>(initialCloud ? 'cloud' : 'lan');
  // The LAN socket is created eagerly so AuthScreen can (auto)connect; the cloud
  // connection is constructed by CloudAuthScreen only after the E2EE + TOTP gate.
  let lanWs = $state<RemoteConnection | null>(initialLan);
  let ws = $state<RemoteLink | null>(initialLan);
  let verified = $state(false);
  let transportSet = $state(false);

  // §perf 懒加载 auth screen 和 MainApp
  let AuthScreenComp = $state<import('svelte').Component<{ ws: RemoteConnection; onverified: () => void }> | null>(null);
  let CloudAuthScreenComp = $state<import('svelte').Component<{ onready: (conn: RemoteLink) => void; onfallbacklan: () => void }> | null>(null);
  let MainAppComp = $state<import('svelte').Component<{ ws: RemoteLink }> | null>(null);

  $effect(() => {
    if (mode === 'cloud' && !verified && !CloudAuthScreenComp) {
      void import('./CloudAuthScreen.svelte').then((m) => { CloudAuthScreenComp = m.default; });
    } else if (mode === 'lan' && !verified && !AuthScreenComp) {
      void import('./AuthScreen.svelte').then((m) => { AuthScreenComp = m.default; });
    }
    if (verified && ws && !MainAppComp) {
      void import('./MainApp.svelte').then((m) => { MainAppComp = m.default; });
    }
  });

  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        refetchOnWindowFocus: false,
        // Remote owns reconnect/backoff at the transport layer. Retrying a
        // failed Git/File query here would duplicate an RPC while the socket
        // is already recovering; explicit refresh remains available in the
        // sidebar and uses the same Query key/single-flight path.
        retry: false,
        refetchOnReconnect: false,
      },
    },
  });

  // LAN sidebar transport. (Cloud sets TauriDataProvider inside cloudControllerBoot,
  // so we must NOT install WsDataProvider in cloud mode.)
  $effect(() => {
    if (mode === 'lan' && verified && lanWs && !transportSet) {
      setTransport(new WsDataProvider(lanWs));
      transportSet = true;
    }
  });

  function fallbackToLan() {
    lanWs = new RemoteConnection();
    ws = lanWs;
    mode = 'lan';
  }

  function handleLanVerified() {
    if (!lanWs) return;
    bridge.attach(createLanWsTransport(lanWs), { useGlobalWorkspace: false });
    verified = true;
  }
</script>

<QueryClientProvider client={queryClient}>
  {#if mode === 'cloud'}
    {#if !verified}
      {#if CloudAuthScreenComp}
        <CloudAuthScreenComp
          onready={(conn) => { ws = conn; verified = true; }}
          onfallbacklan={fallbackToLan}
        />
      {:else}
        <div class="flex items-center justify-center min-h-[100dvh]">…</div>
      {/if}
    {:else if ws && MainAppComp}
      <MainAppComp {ws} />
    {:else if ws}
      <div class="flex items-center justify-center min-h-[100dvh]">…</div>
    {/if}
  {:else if !verified}
    {#if AuthScreenComp}
      <AuthScreenComp ws={lanWs!} onverified={handleLanVerified} />
    {:else}
      <div class="flex items-center justify-center min-h-[100dvh]">…</div>
    {/if}
  {:else if ws && MainAppComp}
    <MainAppComp {ws} />
  {:else if ws}
    <div class="flex items-center justify-center min-h-[100dvh]">…</div>
  {/if}
</QueryClientProvider>
