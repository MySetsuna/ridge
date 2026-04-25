<script lang="ts">
  // Plugin region — iterates the registered plugins for a given scope and
  // mounts each component with the appropriate props. Callers pick the
  // scope; global is typically rendered once at the bottom of the sidebar,
  // workspace inside each workspace group header, pane inside each cwd column.

  import { sidebarPluginStore, type SidebarPluginScope } from '$lib/stores/sidebarPlugins';

  interface Props {
    scope: SidebarPluginScope;
    workspaceId?: string;
    paneId?: string;
    cwd?: string;
  }

  let { scope, workspaceId, paneId, cwd }: Props = $props();

  const matching = $derived(
    $sidebarPluginStore.filter((p) => p.scope === scope)
  );
</script>

{#each matching as plugin (plugin.id)}
  {@const PluginComponent = plugin.component}
  <PluginComponent {workspaceId} {paneId} {cwd} />
{/each}
