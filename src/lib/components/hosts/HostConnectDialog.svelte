<script lang="ts">
  // 连接远端主机对话框（LAN ridge / rdg）。P3/P4 基础层：收集连接参数并登记主机。
  // 凭据(token/TOTP)只发给后端 live 传输里程使用，前端与主机记录都不留存。
  import { Globe, Server, X } from 'lucide-svelte';
  import { connectHost } from '$lib/stores/hosts';
  import { alertDialog } from '../RidgeDialog.svelte';

  interface Props {
    open: boolean;
  }
  let { open = $bindable() }: Props = $props();

  let kind = $state<'remote' | 'rdg'>('remote');
  let label = $state('');
  let addr = $state('');
  let token = $state('');
  let busy = $state(false);

  function reset(): void {
    kind = 'remote';
    label = '';
    addr = '';
    token = '';
  }

  function close(): void {
    reset();
    open = false;
  }

  async function submit(): Promise<void> {
    if (!addr.trim()) {
      await alertDialog({ title: '缺少地址', message: '请填写主机地址（ip:port）。' });
      return;
    }
    busy = true;
    try {
      await connectHost(kind, label, addr, token);
      close();
    } catch (e) {
      await alertDialog({ title: '登记失败', message: e instanceof Error ? e.message : String(e) });
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <div
    class="fixed inset-0 z-[150] flex items-center justify-center bg-black/40 backdrop-blur-sm"
    role="presentation"
    onclick={close}
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="w-[min(440px,92vw)] rounded-2xl border border-[var(--rg-border)] bg-[var(--rg-surface-2)] shadow-2xl text-[var(--rg-fg)]"
      role="dialog"
      aria-label="连接远端主机"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
    >
      <header class="flex items-center justify-between px-4 h-12 border-b border-[var(--rg-border)]">
        <span class="text-[13px] font-semibold">连接远端主机</span>
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded-lg text-[var(--rg-fg-muted)] hover:bg-white/[0.06] hover:text-[var(--rg-fg)]"
          onclick={close}
        >
          <X class="h-4 w-4" />
        </button>
      </header>

      <div class="px-4 py-3 space-y-3">
        <!-- 类型 -->
        <div class="flex gap-2">
          <button
            type="button"
            class="flex-1 flex items-center justify-center gap-1.5 h-9 rounded-lg border text-[12px] transition-colors {kind ===
            'remote'
              ? 'border-[var(--rg-accent)] bg-[var(--rg-accent)]/10 text-[var(--rg-accent)]'
              : 'border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:bg-white/[0.04]'}"
            onclick={() => (kind = 'remote')}
          >
            <Globe class="h-4 w-4" /> 远端 ridge (LAN)
          </button>
          <button
            type="button"
            class="flex-1 flex items-center justify-center gap-1.5 h-9 rounded-lg border text-[12px] transition-colors {kind ===
            'rdg'
              ? 'border-[var(--rg-accent)] bg-[var(--rg-accent)]/10 text-[var(--rg-accent)]'
              : 'border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:bg-white/[0.04]'}"
            onclick={() => (kind = 'rdg')}
          >
            <Server class="h-4 w-4" /> rdg 主机
          </button>
        </div>

        <label class="block">
          <span class="text-[11px] text-[var(--rg-fg-muted)]">地址</span>
          <input
            bind:value={addr}
            placeholder={kind === 'rdg' ? 'host:port（rdg host）' : '192.168.1.5:9528'}
            class="mt-1 w-full h-9 px-2.5 rounded-lg bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[13px] outline-none focus:border-[var(--rg-accent)]"
          />
        </label>

        <label class="block">
          <span class="text-[11px] text-[var(--rg-fg-muted)]">别名（可选）</span>
          <input
            bind:value={label}
            placeholder="工位台机"
            class="mt-1 w-full h-9 px-2.5 rounded-lg bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[13px] outline-none focus:border-[var(--rg-accent)]"
          />
        </label>

        <label class="block">
          <span class="text-[11px] text-[var(--rg-fg-muted)]"
            >{kind === 'rdg' ? 'TOTP / token' : 'token'}（不会被保存）</span
          >
          <input
            bind:value={token}
            type="password"
            placeholder="连接凭据"
            class="mt-1 w-full h-9 px-2.5 rounded-lg bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[13px] outline-none focus:border-[var(--rg-accent)]"
          />
        </label>

        <p class="text-[10px] leading-relaxed text-[var(--rg-fg-muted)]">
          当前版本先登记主机；远端 PTY 流接管（live 传输）为下一里程，需 rebuild + 真实主机联调。
        </p>
      </div>

      <footer class="flex justify-end gap-2 px-4 h-14 items-center border-t border-[var(--rg-border)]">
        <button
          type="button"
          class="h-8 px-3 rounded-lg text-[12px] text-[var(--rg-fg-muted)] hover:bg-white/[0.06]"
          onclick={close}
        >
          取消
        </button>
        <button
          type="button"
          disabled={busy || !addr.trim()}
          class="h-8 px-3 rounded-lg text-[12px] bg-[var(--rg-accent)] text-black font-medium hover:opacity-90 disabled:opacity-40"
          onclick={submit}
        >
          登记主机
        </button>
      </footer>
    </div>
  </div>
{/if}
