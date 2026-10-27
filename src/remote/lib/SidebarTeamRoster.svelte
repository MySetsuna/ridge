<script lang="ts">
  import type { ComponentProps } from 'svelte';
  import GlobalCommuneRoster from './GlobalCommuneRoster.svelte';
  let props: ComponentProps<typeof GlobalCommuneRoster> = $props();
</script>
<GlobalCommuneRoster {...props} />
