import { registerSidebarPlugin } from '$lib/stores/sidebarPlugins';
import GlobalStatusPanel from './globalStatus/GlobalStatusPanel.svelte';

// `workspace-summary` plugin removed: it last showed only "N pane",
// which clutters the workspace header for no real value. The Explorer
// now goes straight from workspace title row to its column list. The
// .svelte file stays on disk in case a future feature wants to repopulate
// the workspace-scope plugin region.

// `native-sessions` always-on sidebar panel removed (2026-06-05): it surfaced
// headless tmux sessions to "summon" into a workspace, but a permanently-visible
// panel that's empty for most users was clutter. The discovery VALUE was kept,
// re-shaped (2026-06-08) as a CONDITIONAL entry folded into GlobalStatusPanel:
// it only renders when ≥1 unattached native session exists, so the common case
// stays zero-clutter. Its `list_native_sessions` / `summon_native_session`
// commands are remote-enabled (in REMOTE_ALLOWLIST); `summon` takes the caller's
// viewed workspace id so the session lands where the remote user is actually
// looking. The headless engine (`teammate/native.rs` → `ridge-tmux`) is also
// reachable via the tmux shim's `attach-session` → `POST /api/v1/tmux/summon`
// (`route_tmux_summon`).

registerSidebarPlugin({
  id: 'global-status',
  title: '全局状态',
  scope: 'global',
  component: GlobalStatusPanel,
  order: 100,
});
