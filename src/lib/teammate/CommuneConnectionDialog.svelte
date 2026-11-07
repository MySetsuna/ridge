<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import { t, tr } from '$lib/i18n';
  import { invoke } from '@tauri-apps/api/core';
  import type { CommuneOrganizationMember } from '../../../packages/remote/src/shared/transport/wsRemote';
  import { connectionRequest } from './communeMembership';

  let { workspaceId, paneId, memberAgentId, onfinish }: {
    workspaceId: string; paneId: string; memberAgentId?: string; onfinish: () => void;
  } = $props();
  type NativeMember = CommuneOrganizationMember & { process?:unknown; source?:{process?:unknown} | null };
  let selectedMember = $state(untrack(() => memberAgentId ?? ''));
  let members = $state<NativeMember[]>([]);
  const previousMember = $derived(members.find(member => member.agentId === selectedMember));
  let dialog: HTMLDialogElement;
  let raw = $state('');
  let source = $state('');
  let request = $state<Record<string, unknown> | null>(null);
  let error = $state('');
  let connecting = $state(false);
  let reading = $state(false);
  let readGeneration = 0;

  onMount(() => {
    dialog.showModal();
    let active = true;
    void invoke<{members?:NativeMember[]}>('get_teammate_topology',{groupScope:'host'})
      .then(projection => { if(active) members=(projection.members ?? []).filter(member => !member.archived); })
      .catch(cause => { if(active) error=String(cause); });
    return () => { active=false; };
  });
  onDestroy(() => { readGeneration++; });

  function preview(text: string): void {
    request = null;
    error = '';
    try { request = connectionRequest(text, workspaceId, paneId); }
    catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
  }

  async function readFile(event: Event): Promise<void> {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    const generation = ++readGeneration;
    request = null;
    raw = '';
    error = '';
    source = file.name;
    reading = true;
    try {
      if (file.size > 65536) throw new Error(tr('commune.connectionTooLarge'));
      const text = await file.text();
      if (generation !== readGeneration) return;
      raw = text;
      preview(text);
    } catch (cause) {
      if (generation === readGeneration) error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      if (generation === readGeneration) reading = false;
    }
  }

  function editDescription(): void {
    readGeneration++;
    reading = false;
    request = null;
    error = '';
    source = tr('commune.connectionPasted');
  }

  function trapFocus(event: KeyboardEvent): void {
    if (event.key !== 'Tab') return;
    const controls = Array.from(dialog.querySelectorAll<HTMLElement>('button:enabled, input:enabled, textarea:enabled, select:enabled, summary'))
      .filter(control => control.getClientRects().length > 0);
    const first = controls[0], last = controls.at(-1);
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  }

  async function connect(): Promise<void> {
    if (!request || connecting || reading) return;
    connecting = true;
    error = '';
    try {
      const {memberAgentId: _descriptorMember, ...descriptor} = request;
      await invoke('register_commune_member', { ...descriptor, ...(selectedMember ? {memberAgentId:selectedMember} : {}) });
      onfinish();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
      connecting = false;
    }
  }
</script>

<dialog bind:this={dialog} aria-labelledby="commune-connect-title" onkeydown={trapFocus} oncancel={(event) => {
  event.preventDefault();
  if (!connecting) onfinish();
}}>
  <header>
    <span class="eyebrow">Agent's Commune</span>
    <h2 id="commune-connect-title">{$t('commune.connectExistingAgent')}</h2>
    <p>{$t('commune.connectionIntro')}</p>
  </header>
  <div class="body">
    <label class="file-picker">
      <span>{$t('commune.connectionFile')}</span>
      <span class="hint">{$t('commune.connectionFormat')}</span>
      <input type="file" accept=".json,application/json" aria-label={$t('commune.connectionFile')} onchange={readFile} disabled={connecting} />
    </label>
    <p class="hint">{$t('commune.connectionHint')}</p>
    {#if source}<p class="source">{reading ? $t('commune.connectionReading') : $t('commune.connectionSource') } · {source}</p>{/if}
    {#if members.length}
      <label for="commune-member">{$t('commune.connectionReuseMember')}</label>
      <select id="commune-member" bind:value={selectedMember} disabled={connecting}>
        <option value="">{$t('commune.connectionAutomatic')}</option>
        {#each members as member (member.agentId)}<option value={member.agentId} disabled={member.protocolConnected}>{member.name} · {member.workspaceId ?? $t('commune.connectionHistorical') }{member.protocolConnected ? ' · ' + $t('commune.connectionStillLive') : ''}</option>{/each}
      </select>
      <p class="hint">{$t('commune.connectionRebindHint')}</p>
    {/if}
    {#if request}
      <dl aria-label={$t('commune.connectionPreview')}>
        <div><dt>{$t('commune.connectionProtocol')}</dt><dd>{String(request.protocol ?? 'acp')}</dd></div>
        <div><dt>{$t('commune.connectionSession')}</dt><dd>{String(request.sessionId ?? $t('commune.connectionUnprovided'))}</dd></div>
        <div><dt>{$t('commune.connectionCwd')}</dt><dd>{String(request.cwd ?? $t('commune.connectionUnprovided'))}</dd></div>
        <div><dt>{$t('commune.connectionPane')}</dt><dd>{paneId}</dd></div>
        <div><dt>{$t('commune.connectionProcess')}</dt><dd>{JSON.stringify(request.process ?? $t('commune.connectionUnprovided'))}</dd></div>
        {#if previousMember}
          <div><dt>{$t('commune.connectionFormerMember')}</dt><dd>{previousMember.name} · {previousMember.agentId}</dd></div>
          <div><dt>{$t('commune.connectionFormerProcess')}</dt><dd>{JSON.stringify(previousMember.process ?? previousMember.source?.process ?? $t('commune.connectionUnrecorded'))}</dd></div>
          <div><dt>{$t('commune.connectionFormerSource')}</dt><dd>{previousMember.workspaceId ?? $t('commune.connectionUnrecorded') } · {previousMember.paneId ?? $t('commune.connectionUnrecorded') }</dd></div>
          <div><dt>{$t('commune.connectionFormerSession')}</dt><dd>{previousMember.sessionId ?? $t('commune.connectionNotAttached') }</dd></div>
          <div><dt>{$t('commune.connectionFormerGeneration')}</dt><dd>{previousMember.generation ?? $t('commune.connectionNotAttached') }</dd></div>
        {/if}
      </dl>
    {/if}
    <details>
      <summary>{$t('commune.connectionAdvanced')}</summary>
      <label for="commune-description">{$t('commune.connectionJson')}</label>
      <textarea id="commune-description" bind:value={raw} oninput={editDescription} disabled={connecting} spellcheck="false"></textarea>
      <button type="button" onclick={() => preview(raw)} disabled={connecting || reading || !raw.trim()}>{$t('commune.connectionPreviewAction')}</button>
    </details>
    {#if error}
      <section class="error" role="alert">
        <strong>{$t('commune.connectionFailure')}</strong>
        <p>{error}</p>
        <p>{$t('commune.connectionRetryHint')}</p>
      </section>
    {/if}
  </div>
  <footer>
    <span aria-live="polite">{connecting ? $t('commune.connectionValidating') : $t('commune.connectionCapabilities') }</span>
    <button type="button" onclick={onfinish} disabled={connecting}>{$t('commune.connectionCancel')}</button>
    <button type="button" class="primary" onclick={connect} disabled={!request || connecting || reading}>{connecting ? $t('commune.connectionConnecting') : $t('commune.connectionConnect') }</button>
  </footer>
</dialog>

<style>
  dialog { width: min(560px, calc(100vw - 32px)); max-height: calc(100dvh - 32px); margin: auto; padding: 0; color: var(--rg-fg); background: var(--rg-surface); border: 1px solid var(--rg-border); border-radius: 14px; box-shadow: 0 24px 80px #0006; overflow: auto; }
  dialog::backdrop { background: #0007; }
  header { padding: 24px 24px 16px; border-bottom: 1px solid var(--rg-border); }
  .eyebrow { color: var(--rg-fg-muted); font-size: 11px; letter-spacing: .08em; }
  h2 { margin: 8px 0; font-size: 18px; font-weight: 600; }
  p { margin: 8px 0 0; line-height: 1.6; font-size: 13px; overflow-wrap: anywhere; }
  .body { padding: 20px 24px; display: grid; gap: 14px; }
  .file-picker { position: relative; display: grid; gap: 10px; padding: 16px; border: 1px dashed var(--rg-border); border-radius: 9px; font-size: 13px; cursor: pointer; }
  .file-picker:focus-within { outline: 2px solid var(--rg-accent); outline-offset: 3px; }
  input { position: absolute; inset: 0; width: 100%; height: 100%; opacity: 0; cursor: pointer; }
  .hint, .source { color: var(--rg-fg-muted); margin: 0; font-size: 12px; }
  dl { margin: 0; padding: 12px; background: var(--rg-bg); border: 1px solid var(--rg-border); border-radius: 9px; font-size: 12px; }
  dl div { display: grid; grid-template-columns: 70px minmax(0, 1fr); gap: 10px; padding: 5px 0; }
  dt { color: var(--rg-fg-muted); } dd { margin: 0; overflow-wrap: anywhere; }
  summary { cursor: pointer; min-height: 30px; font-size: 12px; color: var(--rg-fg-muted); }
  details label { display: block; font-size: 12px; margin: 10px 0; }
  textarea { width: 100%; box-sizing: border-box; min-height: 130px; resize: vertical; padding: 10px; color: var(--rg-fg); background: var(--rg-bg); border: 1px solid var(--rg-border); border-radius: 7px; font: 12px/1.5 var(--rg-font-mono, monospace); }
  select { width:100%; min-width:0; max-width:100%; box-sizing:border-box; min-height:40px; color:var(--rg-fg); background:var(--rg-bg); border:1px solid var(--rg-border); border-radius:7px; }
  button { min-height: 40px; border: 1px solid var(--rg-border); border-radius: 7px; padding: 8px 14px; background: var(--rg-bg); color: var(--rg-fg); cursor: pointer; font-size: 12px; }
  button:disabled { opacity: .45; cursor: default; }
  .primary { background: var(--rg-accent); color: var(--rg-bg); border-color: var(--rg-accent); }
  :is(button, input, textarea, summary):focus-visible { outline: 2px solid var(--rg-accent); outline-offset: 3px; }
  footer { padding: 16px 24px; border-top: 1px solid var(--rg-border); display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: 8px; }
  footer span { flex: 1 1 140px; color: var(--rg-fg-muted); font-size: 11px; }
  .error { border-left: 2px solid var(--rg-ansi-red, #c77d6c); padding: 10px 12px; background: var(--rg-bg); }
  .error strong { font-size: 13px; }
  @media (max-width: 600px) { button, summary { min-height: 44px; } header, .body, footer { padding-left: 16px; padding-right: 16px; } }
</style>
