<script lang="ts">
  import AuthScreen from './AuthScreen.svelte';
  import CloudAuthScreen from './CloudAuthScreen.svelte';
  import MainApp from './MainApp.svelte';
  import { RemoteConnection, type RemoteLink } from './lib/wsRemote';
  import { setTransport } from '$lib/transport';
  import { WsDataProvider } from '$lib/transport/ws';

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
</script>

{#if mode === 'cloud'}
  {#if !verified}
    <CloudAuthScreen
      onready={(conn) => { ws = conn; verified = true; }}
      onfallbacklan={fallbackToLan}
    />
  {:else if ws}
    <MainApp {ws} />
  {/if}
{:else if !verified}
  <AuthScreen ws={lanWs!} onverified={() => verified = true} />
{:else if ws}
  <MainApp {ws} />
{/if}
