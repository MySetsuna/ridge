import { registerSidebarPlugin } from '$lib/stores/sidebarPlugins';
import GlobalStatusPanel from './globalStatus/GlobalStatusPanel.svelte';

// `workspace-summary` plugin removed: it last showed only "N pane",
// which clutters the workspace header for no real value. The Explorer
// now goes straight from workspace title row to its column list. The
// .svelte file stays on disk in case a future feature wants to repopulate
// the workspace-scope plugin region.

// `native-sessions` plugin removed (2026-06-05): the sidebar panel surfaced
// external tmux sessions to "summon" into a workspace, but it was a redundant
// user-facing surface — over web-remote it was already gated off as a dead zone
// (its `summon_native_session` command is excluded from the remote allowlist),
// and on the desktop it duplicated workspace/session management most users never
// touched. The backing headless tmux engine (`teammate/native.rs`) and its
// `list_native_sessions` / `summon_native_session` commands stay — they are core
// to teammate/remote/PTY orchestration; only the redundant panel is gone.

registerSidebarPlugin({
  id: 'global-status',
  title: '全局状态',
  scope: 'global',
  component: GlobalStatusPanel,
  order: 100,
});
