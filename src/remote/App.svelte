<script lang="ts">
  import AuthScreen from './AuthScreen.svelte';
  import MainApp from './MainApp.svelte';
  import { RemoteConnection } from './lib/wsRemote';
  import { setTransport } from '$lib/transport';
  import { WsDataProvider } from '$lib/transport/ws';

  let ws = $state(new RemoteConnection());
  let verified = $state(false);
  let transportSet = $state(false);

  $effect(() => {
    if (verified && !transportSet) {
      setTransport(new WsDataProvider(ws));
      transportSet = true;
    }
  });
</script>

{#if !verified}
  <AuthScreen {ws} onverified={() => verified = true} />
{:else}
  <MainApp {ws} />
{/if}
