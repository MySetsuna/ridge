<script lang="ts">
  // 「最小化·后台保活」按钮（契约 §8）—— LAN tab 与 Cloud tab 共用。
  //
  // 行为复用现有 Rust 命令 `enter_deep_root_mode`（hide + 原生通知 + 托盘恢复），
  // 仅 UI 文案从「深根」改为「最小化省资源」。命令内部名保留不变。
  //
  // 注意（Wave 1）：`enter_deep_root_mode` 当前有 `cloud_remote_active` 前置校验，
  // 故仅当本 tab 有「活跃远控会话」时才启用本按钮（由 `active` 控制）。LAN tab 的
  // LAN 活跃复用同一前置在 Rust 侧尚未放宽（需 W2 加 lan_remote_active 旗标或放宽
  // 前置），因此 LAN tab 启用时点击可能返回 NO_ACTIVE_CLOUD_REMOTE → 走 onError。

  import { invoke } from '@tauri-apps/api/core';
  import { Minimize2 } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Props {
    /** 本 tab 是否有活跃远控会话（启用按钮的依据）。 */
    active: boolean;
    /** 命令失败回调（前端据此 toast/inline 报错）。 */
    onError?: (message: string) => void;
  }

  let { active, onError }: Props = $props();

  async function minimize(): Promise<void> {
    try {
      await invoke('enter_deep_root_mode');
    } catch (e) {
      onError?.(e instanceof Error ? e.message : 'minimize failed');
    }
  }
</script>

<button
  onclick={minimize}
  disabled={!active}
  title={active ? $t('cloud.minimizeTipOn') : $t('cloud.minimizeTipOff')}
  class="group flex w-full items-center justify-center gap-2 rounded-xl border py-2.5 text-sm font-medium transition-all disabled:opacity-40
    border-emerald-500/30 text-emerald-400 hover:bg-emerald-500/10 hover:border-emerald-500/50"
>
  <Minimize2 class="h-4 w-4 transition-transform group-hover:scale-110" />
  {$t('cloud.minimizeBtn')}
</button>
<p class="text-center text-[10px] leading-relaxed text-[var(--rg-fg-muted)]">
  {$t('cloud.minimizeHint')}
</p>
