<script lang="ts">
  import { onMount, tick, untrack, type Snippet } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowUp, ArrowDown, CornerDownLeft, ChevronDown, Crosshair, Send, Terminal } from 'lucide-svelte';
  import { autoGrow } from '$lib/actions/autoGrow';
  import { locateTerminal } from './locateTerminal';
  import type { DiscoveredAgent } from './teammateModel';
  import { manualTerminalInput, terminalTextBytes, terminalView, type TerminalView } from './terminalAgent';
  import { t, tr } from '$lib/i18n';

  let { agent, workspaceName, expanded, onToggle, children }: {
    agent: DiscoveredAgent; workspaceName: string; expanded: boolean; onToggle: () => void; children?: Snippet;
  } = $props();
  let view = $state<TerminalView | null>(null);
  let text = $state('');
  let error = $state('');
  let notice = $state('');
  let sending = $state(false);
  let uncertain = $state(false);
  let preview = $state<HTMLPreElement>();
  let scrolledUp = $state(false);
  const previewText = $derived((expanded && view ? view.text : agent.recentOutput || '').trimEnd());
  const visibleText = $derived(expanded ? previewText : previewText.split('\n').slice(-4).join('\n'));
  $effect(() => {
    if (previewText && expanded && preview && untrack(() => !scrolledUp)) {
      const target = preview;
      void tick().then(() => { if (preview === target) target.scrollTop = target.scrollHeight; });
    }
  });
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
        if (!view) throw new Error(tr('commune.terminalWaitingView'));
        const bytes = key ? view.keyBytes[key] : terminalTextBytes(view, text);
        if (!bytes?.length) throw new Error(tr('commune.terminalKeyUnavailable'));
        input.prepare(agent, view, bytes, key ? undefined : view.keyBytes.Enter);
      }
      await input.send(invoke);
      uncertain = false; if (!key) text = ''; notice = tr('commune.terminalInputWritten');
      await refresh();
    } catch (failure) { uncertain = input.pending !== null; error = `${String(failure)}${uncertain ? ` · ${tr('commune.terminalResultUnconfirmed')}` : ''}`; }
    finally { sending = false; }
  }
  async function locate() {
    try { await locateTerminal(agent.workspaceId, agent.paneId); }
    catch (failure) { error = tr('commune.locateFailed', { name: agent.name, error: String(failure) }); }
  }
  function composeKey(event: KeyboardEvent) {
    if (event.key !== 'Enter' || event.shiftKey || event.isComposing || event.ctrlKey || event.altKey || event.metaKey) return;
    event.preventDefault();
    if (view && text.trim() && !sending && !uncertain) void send();
  }
</script>

<li class="terminal-card" class:expanded data-testid="commune-terminal-agent" data-workspace-id={agent.workspaceId} data-pane-id={agent.paneId}>
  <div class="card-heading">
    <span class="agent-icon"><Terminal size={15} strokeWidth={1.6} /></span>
    <button class="identity" onclick={onToggle} aria-expanded={expanded}>
      <span class="agent-name">{agent.name}</span>
      <span class="workspace" title={workspaceName}>{workspaceName}</span>
    </button>
    <button class="locate" onclick={() => void locate()} title={$t('commune.terminalLocateHint')}><Crosshair size={13} />{$t('commune.locateTerminal')}</button>
  </div>
  <div class="output">
    <div class="output-heading"><span>{$t('commune.terminalLatest')}</span><span class="connection">{agent.protocolConnected ? $t('commune.protocolConnected') : $t('commune.terminalDirect')}</span></div>
    <pre bind:this={preview} class="rg-scroll" data-testid="commune-terminal-preview" onscroll={() => { if (preview) scrolledUp = preview.scrollHeight - preview.clientHeight - preview.scrollTop > 12; }}>{visibleText || $t('commune.terminalWaitingOutput')}</pre>
    {#if expanded && scrolledUp}<button class="latest" onclick={() => { scrolledUp = false; if (preview) preview.scrollTop = preview.scrollHeight; }}>{$t('commune.terminalBackToLatest')} <ArrowDown size={11} /></button>{/if}
  </div>
  <button class="toggle" onclick={onToggle} aria-expanded={expanded}>{expanded ? $t('commune.collapse') : $t('commune.expandInteraction')}<ChevronDown size={12} class={expanded ? 'rotate-180' : ''} /></button>
  {#if expanded}
    <div class="interaction">
      <div class="keys" aria-label={$t('commune.terminalKeys')}>
        <span class="key-label">{$t('commune.terminalOptions')}</span>
        <button class="key" aria-label={tr('commune.terminalSendKey', { key: 'ArrowUp' })} title={$t('commune.terminalPrevious')} disabled={!view || sending || uncertain} onclick={() => void send('ArrowUp')}><ArrowUp size={13} /></button>
        <button class="key" aria-label={tr('commune.terminalSendKey', { key: 'ArrowDown' })} title={$t('commune.terminalNext')} disabled={!view || sending || uncertain} onclick={() => void send('ArrowDown')}><ArrowDown size={13} /></button>
        <button class="key confirm" aria-label={tr('commune.terminalSendKey', { key: 'Enter' })} disabled={!view || sending || uncertain} onclick={() => void send('Enter')}><CornerDownLeft size={12} />{$t('commune.confirm')}</button>
        <button class="key" aria-label={tr('commune.terminalSendKey', { key: 'Escape' })} disabled={!view || sending || uncertain} onclick={() => void send('Escape')}>Esc</button>
        <details class="more-keys"><summary>{$t('commune.more')}</summary>
          <div class="key-menu">
            {#each [{key:'ArrowLeft',label:$t('commune.terminalLeft')},{key:'ArrowRight',label:$t('commune.terminalRight')},{key:'Tab',label:$t('commune.terminalTab')},{key:'Backspace',label:$t('commune.terminalBackspace')},{key:'Ctrl+C',label:$t('commune.terminalInterrupt')}] as action}
              <button class="key" aria-label={tr('commune.terminalSendKey', { key: action.key })} disabled={!view || sending || uncertain} onclick={() => void send(action.key)}>{action.label}</button>
            {/each}
          </div>
        </details>
      </div>
      <div class="composer">
        <textarea rows="2" use:autoGrow={{maxRows:6,value:text}} aria-label={tr('commune.terminalReplyAria', { name: agent.name })} placeholder={$t('commune.terminalReplyPlaceholder')} bind:value={text} disabled={sending || uncertain} onkeydown={composeKey}></textarea>
        <div class="composer-footer"><span>{$t('commune.terminalSendHint')}</span><button class="send" aria-label={$t('commune.terminalSendAndEnter')} disabled={!view || !text.trim() || sending || uncertain} onclick={() => void send()}><Send size={12} />{sending ? $t('commune.sending') : $t('commune.send')}</button></div>
      </div>
      {#if uncertain}
        <div class="uncertain">
          <button disabled={sending} onclick={() => void send()}>{$t('commune.confirmOriginalRequest')}</button>
          <button disabled={sending} onclick={() => { input.abandon(); uncertain = false; text = ''; error = ''; notice = tr('commune.terminalCheckAndRetry'); }}>{$t('commune.terminalCheckAndRetry')}</button>
        </div>
      {/if}
      {#if error}<p role="alert" class="feedback error">{error}</p>{:else if notice}<p role="status" class="feedback">{notice}</p>{/if}
      {#if children}<details class="protocol"><summary>{$t('commune.protocolActions')}</summary><ul>{@render children()}</ul></details>{/if}
    </div>
  {:else if error}
    <p role="alert" class="feedback error collapsed-error">{error}</p>
  {/if}
</li>

<style>
  .terminal-card { overflow: visible; border: 1px solid var(--rg-border); border-radius: 8px; background: var(--rg-bg); color: var(--rg-fg); font-size: 12px; }
  .terminal-card.expanded { border-color: color-mix(in srgb, var(--rg-accent) 45%, var(--rg-border)); }
  button, summary { cursor: pointer; }
  button:disabled { cursor: default; opacity: .4; }
  button:focus-visible, summary:focus-visible { outline: 2px solid var(--rg-accent); outline-offset: 2px; }
  .card-heading { display: flex; align-items: center; gap: 8px; padding: 12px 12px 10px; }
  .agent-icon { display: grid; place-items: center; width: 28px; height: 28px; flex-shrink: 0; border-radius: 6px; background: var(--rg-bg-raised); color: var(--rg-accent); }
  .identity { flex: 1; min-width: 0; text-align: left; }
  .agent-name, .workspace { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .agent-name { font-weight: 600; line-height: 18px; }
  .workspace { margin-top: 2px; font-size: 10px; color: var(--rg-fg-muted); }
  .locate { display: flex; align-items: center; gap: 4px; flex-shrink: 0; padding: 5px 4px; border-radius: 4px; color: var(--rg-fg-muted); font-size: 10px; }
  .locate:hover { background: var(--rg-bg-raised); color: var(--rg-fg); }
  .output { position: relative; margin: 0 10px; padding: 8px 9px; border-radius: 5px; background: var(--rg-bg-raised); }
  .output-heading { display: flex; justify-content: space-between; gap: 6px; margin-bottom: 6px; color: var(--rg-fg-muted); font-size: 10px; }
  .connection { font-size: 9px; }
  pre { margin: 0; max-height: 76px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font-family: var(--font-mono-term); font-size: 11px; line-height: 19px; }
  .expanded pre { max-height: 210px; }
  .latest { position: absolute; right: 8px; bottom: 6px; display: flex; align-items: center; gap: 4px; border: 1px solid var(--rg-border); border-radius: 4px; background: var(--rg-bg); padding: 3px 6px; font-size: 10px; color: var(--rg-accent); }
  .toggle { display: flex; align-items: center; justify-content: space-between; width: 100%; padding: 9px 12px; color: var(--rg-fg-muted); font-size: 11px; }
  .toggle:hover { color: var(--rg-accent); }
  .interaction { padding: 0 10px 10px; }
  .keys { display: flex; align-items: center; gap: 4px; margin-bottom: 9px; }
  .key-label { margin-right: 3px; color: var(--rg-fg-muted); font-size: 10px; }
  .key { display: inline-flex; align-items: center; justify-content: center; gap: 4px; min-width: 27px; height: 27px; padding: 0 7px; border: 1px solid var(--rg-border); border-radius: 4px; font-size: 11px; }
  .key:hover:enabled { background: var(--rg-bg-raised); border-color: var(--rg-fg-muted); }
  .confirm { color: var(--rg-accent); }
  .more-keys { position: relative; margin-left: auto; }
  .more-keys summary { padding: 5px 2px; color: var(--rg-fg-muted); font-size: 10px; list-style: none; }
  .key-menu { position: absolute; z-index: 3; right: 0; top: 30px; display: grid; gap: 4px; width: 120px; padding: 6px; border: 1px solid var(--rg-border); border-radius: 6px; background: var(--rg-bg); box-shadow: 0 4px 16px #0002; }
  .composer { overflow: hidden; border: 1px solid var(--rg-border); border-radius: 5px; background: var(--rg-bg); }
  .composer:focus-within { border-color: var(--rg-accent); }
  textarea { display: block; width: 100%; min-height: 54px; resize: none; padding: 9px; background: transparent; border: 0; outline: none; font-size: 12px; line-height: 18px; }
  textarea::placeholder { color: var(--rg-fg-muted); }
  .composer-footer { display: flex; align-items: center; justify-content: space-between; gap: 4px; padding: 0 6px 6px 9px; }
  .composer-footer > span { color: var(--rg-fg-muted); font-size: 9px; }
  .send { display: flex; align-items: center; gap: 5px; padding: 5px 8px; border-radius: 4px; background: var(--rg-accent); color: var(--rg-bg); font-size: 11px; font-weight: 500; }
  .send:hover:enabled { filter: brightness(1.12); }
  .feedback { margin-top: 7px; color: var(--rg-fg-muted); font-size: 10px; line-height: 16px; overflow-wrap: anywhere; }
  .error { color: var(--rg-fg); border-left: 2px solid var(--rg-accent); padding-left: 7px; }
  .collapsed-error { margin: 0 12px 10px; }
  .uncertain { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 8px; color: var(--rg-accent); font-size: 11px; }
  .protocol { margin-top: 9px; padding-top: 7px; border-top: 1px solid var(--rg-border); color: var(--rg-fg-muted); font-size: 10px; }
  .protocol ul { margin-top: 8px; }
</style>
