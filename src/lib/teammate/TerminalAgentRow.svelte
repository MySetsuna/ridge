<script lang="ts">
  import { onMount, tick, untrack, type Snippet } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { focusPane, switchWorkspace } from '$lib/stores/paneTree';
  import type { DiscoveredAgent } from './teammateModel';
  import { manualTerminalInput, terminalTextBytes, terminalView, type TerminalView } from './terminalAgent';

  let { agent, workspaceName, expanded, onToggle, children }: {
    agent: DiscoveredAgent; workspaceName: string; expanded: boolean; onToggle: () => void; children?: Snippet;
  } = $props();
  let view = $state<TerminalView | null>(null);
  let text = $state('');
  let error = $state('');
  let notice = $state('');
  let sending = $state(false);
  let uncertain = $state(false);
  let busy = false, disposed = false;
  const input = untrack(() => manualTerminalInput(agent));
  async function refresh() {
    if (!expanded || busy || disposed || document.hidden || !agent.terminalEpoch) return;
    busy = true;
    try {
      const result = terminalView(await invoke('get_agent_terminal_view', {
        workspaceId: agent.workspaceId, paneId: agent.paneId, process: agent.process, terminalEpoch: agent.terminalEpoch,
      }), agent);
      if (!disposed) { view = result; if (!uncertain) error = ''; }
    } catch (failure) { if (!disposed) { view = null; error = String(failure); } }
    finally { busy = false; }
  }
  $effect(() => { if (expanded) void refresh(); });
  onMount(() => {
    disposed = false; uncertain = input.pending !== null;
    const timer = setInterval(() => void refresh(), 700);
    return () => { disposed = true; clearInterval(timer); };
  });
  async function send(key?: string) {
    if (sending) return;
    notice = ''; error = '';
    sending = true;
    try {
      if (!input.pending) {
        if (!view) throw new Error('请等待终端视图刷新');
        const bytes = key ? view.keyBytes[key] : terminalTextBytes(view, text);
        if (!bytes?.length) throw new Error('此按键暂不可用');
        input.prepare(agent, view, bytes, key ? undefined : view.keyBytes.Enter);
      }
      await input.send(invoke);
      uncertain = false; if (!key) text = ''; notice = '输入已写入原终端';
      await refresh();
    } catch (failure) { uncertain = input.pending !== null; error = `${String(failure)}${uncertain ? '；结果未确认，请查看终端或确认原请求。' : ''}`; }
    finally { sending = false; }
  }
  async function locate() { await switchWorkspace(agent.workspaceId); focusPane(agent.paneId, agent.workspaceId); await tick(); }
</script>

<li class="rounded-lg border border-[var(--rg-border)] px-3 py-3" data-testid="commune-terminal-agent" data-workspace-id={agent.workspaceId} data-pane-id={agent.paneId}>
  <div class="flex items-center justify-between gap-2">
    <button class="min-w-0 text-left" onclick={onToggle} aria-expanded={expanded}>
      <span class="block truncate text-[12px] font-medium">{agent.name}</span>
      <span class="block truncate text-[10px] text-[var(--rg-fg-muted)]">{workspaceName} · {agent.protocolConnected ? '终端 · 协议已连接' : '终端交互'}</span>
    </button>
    <button class="shrink-0 text-[11px] text-[var(--rg-accent)]" onclick={() => void locate()}>定位终端</button>
  </div>
  <p class="mt-2 text-[10px] text-[var(--rg-fg-muted)]">终端最新内容</p>
  <pre class="rg-scroll mt-1 max-h-36 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-5" data-testid="commune-terminal-preview">{expanded && view ? view.text : agent.recentOutput || '等待终端输出'}</pre>
  <button class="mt-2 text-[11px] text-[var(--rg-accent)]" onclick={onToggle}>{expanded ? '收起交互' : '展开交互'}</button>
  {#if expanded}
    <div class="mt-3 flex flex-wrap gap-1" aria-label="原终端按键">
      {#each [{key:'ArrowUp',label:'↑'},{key:'ArrowDown',label:'↓'},{key:'ArrowLeft',label:'←'},{key:'ArrowRight',label:'→'},{key:'Enter',label:'回车'},{key:'Tab',label:'Tab'},{key:'Escape',label:'Esc'},{key:'Backspace',label:'退格'},{key:'Ctrl+C',label:'Ctrl+C'}] as action}
        <button class="rounded border border-[var(--rg-border)] px-2 py-1 text-[11px] disabled:opacity-40" aria-label={`向原终端发送 ${action.key}`} disabled={!view || sending || uncertain} onclick={() => void send(action.key)}>{action.label}</button>
      {/each}
    </div>
    <textarea class="mt-2 min-h-20 w-full resize-y rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] p-2 text-[12px]" aria-label={`回复 ${agent.name} 的原终端`} placeholder="输入消息或选项回复" bind:value={text} disabled={sending || uncertain}></textarea>
    <button class="mt-1 rounded bg-[var(--rg-accent)]/15 px-3 py-1.5 text-[11px] disabled:opacity-40" disabled={!view || !text.trim() || sending || uncertain} onclick={() => void send()}>发送并回车</button>
    {#if uncertain}
      <div class="mt-2 flex flex-wrap gap-3 text-[11px]">
        <button disabled={sending} onclick={() => void send()}>确认原请求</button>
        <button disabled={sending} onclick={() => { input.abandon(); uncertain = false; text = ''; error = ''; notice = '请查看原终端后重新输入'; }}>已检查终端，重新输入</button>
      </div>
    {/if}
    {#if children}<details class="mt-3"><summary class="cursor-pointer text-[11px] text-[var(--rg-fg-muted)]">协议操作</summary><ul class="mt-2">{@render children()}</ul></details>{/if}
  {/if}
  {#if error}<p role="alert" class="mt-2 break-all text-[11px] text-amber-400">{error}</p>{/if}
  {#if notice}<p role="status" class="mt-2 text-[11px] text-[var(--rg-fg-muted)]">{notice}</p>{/if}
</li>
