<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { connectionRequest } from './communeMembership';

  let { workspaceId, paneId, onfinish }: {
    workspaceId: string; paneId: string; onfinish: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let raw = $state('');
  let source = $state('');
  let request = $state<Record<string, unknown> | null>(null);
  let error = $state('');
  let connecting = $state(false);
  let reading = $state(false);
  let readGeneration = 0;

  onMount(() => dialog.showModal());
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
      if (file.size > 65536) throw new Error('连接描述过大，最多 64 KiB');
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
    source = '粘贴的连接描述';
  }

  function trapFocus(event: KeyboardEvent): void {
    if (event.key !== 'Tab') return;
    const controls = Array.from(dialog.querySelectorAll<HTMLElement>('button:enabled, input:enabled, textarea:enabled, summary'))
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
      await invoke('register_commune_member', request);
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
    <h2 id="commune-connect-title">连接既有 Agent 会话</h2>
    <p>选择运行方提供的会话连接文件，确认后接入身份、对话和协作能力。</p>
  </header>
  <div class="body">
    <label class="file-picker">
      <span>选择连接文件</span>
      <span class="hint">JSON · 最大 64 KiB</span>
      <input type="file" accept=".json,application/json" aria-label="选择连接文件" onchange={readFile} disabled={connecting} />
    </label>
    <p class="hint">使用原 Agent 已有会话的连接描述，最大 64 KiB。普通终端须由运行方提供可接入端点。</p>
    {#if source}<p class="source">{reading ? '正在读取' : '连接来源'} · {source}</p>{/if}
    {#if request}
      <dl aria-label="会话预览">
        <div><dt>协议</dt><dd>{String(request.protocol ?? 'acp')}</dd></div>
        <div><dt>已有会话</dt><dd>{String(request.sessionId ?? '未提供，连接时核验')}</dd></div>
        <div><dt>工作目录</dt><dd>{String(request.cwd ?? '未提供，连接时核验')}</dd></div>
        <div><dt>目标分屏</dt><dd>{paneId}</dd></div>
      </dl>
    {/if}
    <details>
      <summary>高级 · 粘贴连接描述</summary>
      <label for="commune-description">运行方提供的 JSON</label>
      <textarea id="commune-description" bind:value={raw} oninput={editDescription} disabled={connecting} spellcheck="false"></textarea>
      <button type="button" onclick={() => preview(raw)} disabled={connecting || reading || !raw.trim()}>预览描述</button>
    </details>
    {#if error}
      <section class="error" role="alert">
        <strong>连接未完成</strong>
        <p>{error}</p>
        <p>核对原 Agent 仍在运行、连接文件及授权有效后重试。输入已保留。</p>
      </section>
    {/if}
  </div>
  <footer>
    <span aria-live="polite">{connecting ? '正在核验进程与会话…' : '连接能力以 Host 核验结果为准'}</span>
    <button type="button" onclick={onfinish} disabled={connecting}>取消</button>
    <button type="button" class="primary" onclick={connect} disabled={!request || connecting || reading}>{connecting ? '连接中…' : '连接'}</button>
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
