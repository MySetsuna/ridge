// src/lib/plugins/index.ts
//
// Side-effect module: importing this once (from +page.svelte) boots every
// built-in sidebar plugin. Registration lives here — not inside the plugin
// components — so components don't have to self-import (which breaks under
// the Vite module pipeline).
//
// Round 34 removed the `claude-history` per-pane plugin: Claude Code lives
// exclusively in its own sidebar tab now (`ClaudeCodePanel.svelte`,
// added round 27). The plugin's history list functionality was already
// duplicated there per-pane, so dropping it from Explorer is a clean
// removal — the underlying `claudeHistory/store.ts` stays as the data
// source the new tab consumes.
//
// `ClaudeHistoryPanel.svelte` is intentionally kept on disk in case a
// future power-user wants to re-register it as a personal plugin; the
// orphan file pays no runtime cost.

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
