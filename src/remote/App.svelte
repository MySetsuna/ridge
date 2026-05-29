<script lang="ts">
  import AuthScreen from './AuthScreen.svelte';
  import MainApp from './MainApp.svelte';
  import { RemoteConnection } from './lib/wsRemote';

  let ws = $state(new RemoteConnection());
  let verified = $state(false);
</script>

{#if !verified}
  <AuthScreen {ws} onverified={() => verified = true} />
{:else}
  <MainApp {ws} />
{/if}
