<script lang="ts">
  import './communeVisual.css';
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { t } from '$lib/i18n';
  import { nativeCommuneLaunch as launch, type LaunchOptions, type LaunchReceipt } from './communeLaunch';

  let { workspaceId, paneId, onfinish }: {
    workspaceId?: string; paneId?: string; onfinish: (receipt?: LaunchReceipt) => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let options = $state<LaunchOptions>();
  let profile = $state('');
  let cwd = $state('');
  let reading = $state(true);
  let error = $state('');
  const selected = $derived(options?.profiles.find(option => option.id === profile));

  onMount(() => {
    dialog.showModal();
    let active = true;
    void invoke<LaunchOptions>('get_collaboration_launch_options', { workspaceId, paneId })
      .then(value => {
        if (!active) return;
        options = value;
        profile = $launch.pending?.collaboration.profile ?? '';
        cwd = $launch.pending?.cwd ?? value.cwd ?? '';
      }).catch(cause => { if (active) error = String(cause); })
      .finally(() => { if (active) reading = false; });
    return () => { active = false; };
  });

  async function start() {
    if (!options || !profile || !cwd.trim()) return;
    const receipt = await launch.send(request => invoke('create_independent_terminal', { ...request }), {
      profile, cwd: cwd.trim(), runtimeEpoch: options.runtimeEpoch,
    });
    if (receipt) onfinish(receipt);
  }
</script>

<dialog class="rg-commune" bind:this={dialog} aria-labelledby="commune-start-title" oncancel={event => {
  event.preventDefault();
  if (!$launch.busy) onfinish();
}}>
  <header>
    <span class="eyebrow">Agent's Commune</span>
    <h2 id="commune-start-title">{$t('commune.launchTitle')}</h2>
    <p>{$t('commune.launchIntro')}</p>
  </header>
  <div class="body" aria-busy={reading || $launch.busy}>
    <label for="commune-launch-profile">{$t('commune.launchAgent')}</label>
    <select id="commune-launch-profile" bind:value={profile} disabled={reading || !$launch.editable}>
      <option value="">{$t('commune.launchChoose')}</option>
      {#each options?.profiles ?? [] as option (option.id)}<option value={option.id}>{option.name ?? option.id}</option>{/each}
    </select>
    {#if selected}<code>{selected.collaboration.command} {selected.collaboration.args.join(' ')}</code>{/if}
    <label for="commune-launch-cwd">{$t('commune.connectionCwd')}</label>
    <input id="commune-launch-cwd" bind:value={cwd} title={cwd} disabled={reading || !$launch.editable} spellcheck="false" autocomplete="off" />
    <p class="hint">{$t('commune.launchLifecycle')}</p>
    {#if $launch.pending}
      <aside>
        <p>{$t($launch.editable ? 'commune.launchCorrect' : 'commune.launchUnknown')}</p>
        <code>{$launch.pending.workspaceId}<br />{$launch.pending.paneId}</code>
      </aside>
    {/if}
    {#if error || $launch.error}<p class="error" role="alert">{error || $launch.error}</p>{/if}
    {#if !reading && !options?.profiles.length}<p>{$t('commune.launchNoProfiles')}</p>{/if}
  </div>
  <footer>
    <button type="button" onclick={() => onfinish()} disabled={$launch.busy}>{$t('commune.connectionCancel')}</button>
    <button type="button" class="primary" onclick={start} disabled={reading || $launch.busy || !options || !profile || !cwd.trim()}>
      {$t($launch.busy ? 'commune.launchStarting' : $launch.pending ? 'commune.launchRetry' : 'commune.launchCreate')}
    </button>
  </footer>
</dialog>

<style>
  dialog { width: min(500px, calc(100vw - 32px)); max-height: calc(100dvh - 32px); margin: auto; padding: 0; color: var(--rg-fg); background: var(--rg-surface); border: 1px solid var(--rg-border); border-radius: 14px; box-shadow: 0 24px 80px #0006; overflow: auto; }
  dialog::backdrop { background: #0007; }
  header { padding: 24px 24px 18px; border-bottom: 1px solid var(--rg-border); }
  .eyebrow { color: var(--rg-fg-muted); font-size: 11px; letter-spacing: .08em; }
  h2 { margin: 8px 0; font-size: 18px; font-weight: 600; }
  p { margin: 8px 0 0; font-size: 13px; line-height: 1.6; overflow-wrap: anywhere; }
  .body { padding: 20px 24px 24px; display: grid; gap: 10px; }
  label { font-size: 12px; font-weight: 600; }
  input, select { width: 100%; min-height: 40px; padding: 8px 10px; border: 1px solid var(--rg-border); border-radius: 7px; background: var(--rg-bg); color: var(--rg-fg); font-size: 13px; }
  code { display: block; overflow-wrap: anywhere; white-space: pre-wrap; color: var(--rg-fg-muted); font: 11px/1.6 var(--rg-font-mono, monospace); }
  .hint { color: color-mix(in srgb, var(--rg-fg) 65%, var(--rg-fg-muted)); }
  aside { padding: 10px 12px; background: var(--rg-bg); border-left: 2px solid var(--rg-accent); }
  aside p { margin: 0 0 8px; }
  .error { border-left: 2px solid var(--rg-ansi-red, #c77d6c); padding-left: 12px; }
  footer { display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 8px; padding: 16px 24px; border-top: 1px solid var(--rg-border); }
  button { min-height: 38px; padding: 8px 14px; border: 1px solid var(--rg-border); border-radius: 7px; color: var(--rg-fg); font-size: 12px; background: transparent; transition: background .15s; }
  button:hover:enabled { background: color-mix(in srgb, var(--rg-fg) 8%, transparent); }
  button.primary { background: var(--rg-accent); color: var(--rg-bg); border-color: transparent; }
  button.primary:hover:enabled { background: color-mix(in srgb, var(--rg-accent) 85%, var(--rg-bg)); }
  button:disabled { opacity: .5; cursor: default; }
  :is(button, input, select):focus-visible { outline: 2px solid var(--rg-accent); outline-offset: 3px; }
  @media (max-width: 600px) { button, input, select { min-height: 44px; } header, .body, footer { padding-left: 16px; padding-right: 16px; } }
</style>
