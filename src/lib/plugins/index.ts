import { registerSidebarPlugin } from '$lib/stores/sidebarPlugins';
import GlobalStatusPanel from './globalStatus/GlobalStatusPanel.svelte';

// `workspace-summary` plugin removed: it last showed only "N pane",
// which clutters the workspace header for no real value. The Explorer
// now goes straight from workspace title row to its column list. The
// .svelte file stays on disk in case a future feature wants to repopulate
// the workspace-scope plugin region.

registerSidebarPlugin({
  id: 'global-status',
  title: '全局状态',
  scope: 'global',
  component: GlobalStatusPanel,
  order: 100,
});
