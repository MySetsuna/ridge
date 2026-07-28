<script lang="ts">
  // iter-63：手机端「切换终端类型」入口（PS → WSL / Git Bash …）。
  //
  // 列表**与桌面同源**：走 host 的 `detect_available_shells`（RemoteLink.listShells），
  // 客户端不另攒一份候选，否则两端会漂移（用户明确要求「终端类型列表对齐桌面端」）。
  // 单独成组件而非塞进 WorkspaceTree：那文件已 600 行上限在望，且这块自成一个职责
  // （拉列表 → 弹选择 → 切换 → 回报）。
  import { Terminal } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import type { RemoteLink, RemoteShellInfo } from '@ridge/remote';

  interface Props {
    ws: RemoteLink | null | undefined;
    workspaceId: string;
    paneId: string;
    /** 切换成功后让父组件刷新该工作区的终端列表。 */
    onswitched?: () => void;
  }
  let { ws, workspaceId, paneId, onswitched }: Props = $props();

  let open = $state(false);
  let shells = $state<RemoteShellInfo[]>([]);
  let busy = $state(false);
  let loading = $state(false);
  let err = $state('');

  async function toggle(e: Event) {
    e.stopPropagation();
    if (busy) return;
    if (open) {
      open = false;
      return;
    }
    open = true;
    err = '';
    if (shells.length) return;
    // 「连接压根没有这个能力」和「主机上一个 shell 都没检测到」是两回事，
    // 混成同一句「未检测到可用终端」会把排查引到完全错误的方向。
    if (typeof ws?.listShells !== 'function') {
      err = '当前连接不支持切换终端类型（主机版本过旧）';
      return;
    }
    // 拉列表期间必须有 loading 态：`detect_available_shells` 要枚举 WSL 发行版，
    // 可能好几秒；没有它这段时间面板会显示「未检测到可用终端」，是彻底的误报。
    loading = true;
    try {
      shells = await ws.listShells();
    } catch (e2) {
      // 老 host 的 allowlist 没有这条命令 → 明确报错，不静默留个空列表让用户干瞪眼。
      err = e2 instanceof Error ? e2.message : String(e2);
    } finally {
      loading = false;
    }
  }

  async function pick(e: Event, shell: RemoteShellInfo) {
    e.stopPropagation();
    if (busy || !ws?.changePaneShell) return;
    busy = true;
    err = '';
    try {
      await ws.changePaneShell(workspaceId, paneId, shell);
      open = false;
      onswitched?.();
    } catch (e2) {
      err = e2 instanceof Error ? e2.message : $t('mobile.shellSwitchFail');
    } finally {
      busy = false;
    }
  }
</script>

<span
  class="row-shell"
  class:on={open}
  role="button"
  tabindex="-1"
  onclick={toggle}
  onkeydown={() => {}}
  title={$t('mobile.shellSwitch')}
  aria-label={$t('mobile.shellSwitch')}
>
  <Terminal class="w-3 h-3" />
</span>

{#if open}
  <!-- 就近展开的小面板：手机上比全屏 sheet 更快，且不夺走终端列表的上下文。 -->
  <div class="shell-menu" role="menu" tabindex="-1">
    <div class="shell-head">{$t('mobile.shellPickTitle')}</div>
    {#if err}
      <div class="shell-err">{err}</div>
    {:else if loading}
      <div class="shell-note">{$t('mobile.loading')}</div>
    {:else if busy}
      <div class="shell-note">{$t('mobile.shellSwitching')}</div>
    {:else if shells.length === 0}
      <div class="shell-note">{$t('mobile.shellEmpty')}</div>
    {:else}
      {#each shells as s (s.id)}
        <button class="shell-item" onclick={(e) => pick(e, s)}>{s.label}</button>
      {/each}
    {/if}
  </div>
{/if}

<style>
  .row-shell{display:flex;align-items:center;justify-content:center;width:20px;height:20px;border:0;border-radius:0;background:transparent;color:var(--rg-fg-muted);opacity:.7;flex-shrink:0}
  .row-shell.on,.row-shell:active{color:var(--rg-accent);opacity:1;background:transparent}
  /* 脱流挂在 .pane-item（WorkspaceTree 里 position:relative）下沿，
     否则它会被当成 flex 行的一个子项把行撑变形。 */
  .shell-menu{position:absolute;top:100%;left:24px;right:8px;z-index:20;border:1px solid var(--rg-border);border-radius:8px;background:var(--rg-surface);box-shadow:0 6px 18px -4px rgba(0,0,0,.55);overflow:hidden}
  .shell-head{padding:6px 10px;font-size:10px;color:var(--rg-fg-muted);border-bottom:1px solid var(--rg-border)}
  .shell-item{display:block;width:100%;padding:9px 10px;border:0;background:transparent;color:var(--rg-fg);font-size:12px;text-align:left}
  .shell-item:active{background:color-mix(in srgb,var(--rg-accent) 18%,transparent)}
  .shell-note{padding:9px 10px;font-size:11px;color:var(--rg-fg-muted)}
  .shell-err{padding:9px 10px;font-size:11px;color:var(--rg-ansi-red,#f85149)}
</style>
