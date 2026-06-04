import { registerSidebarPlugin } from '$lib/stores/sidebarPlugins';
import GlobalStatusPanel from './globalStatus/GlobalStatusPanel.svelte';
import NativeSessionsPanel from './nativeSessions/NativeSessionsPanel.svelte';

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

// `native-sessions` surfaces the headless tmux engine (teammate/native.rs) and
// its summon-into-workspace action. Both backing commands (`list_native_sessions`
// / `summon_native_session`) are host-only — the latter is explicitly excluded
// from the remote invoke allowlist (remote/server.rs), and the former has no
// allowlist arm — so over web-remote the panel can only ever render an empty,
// non-actionable section. Gate it to the desktop build to avoid that dead zone.
const webRemote = import.meta.env.RIDGE_WEB_REMOTE === true;

if (!webRemote) {
  registerSidebarPlugin({
    id: 'native-sessions',
    title: 'Native 会话',
    scope: 'global',
    component: NativeSessionsPanel,
    order: 90,
  });
}
