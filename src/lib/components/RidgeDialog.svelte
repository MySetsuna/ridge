<script lang="ts" module>
  // src/lib/components/WindDialog.svelte
  //
  // Themed replacement for `window.alert` / `window.confirm` /
  // `window.prompt`. Native dialogs in the Tauri WebView render with OS
  // chrome — wrong font, wrong colors, sometimes blocked entirely on
  // Windows when the window doesn't own focus — and they break the
  // visual coherence the rest of the app maintains.
  //
  // API: three module-level promise-returning helpers. The returned
  // promise resolves when the user clicks a button or presses Esc/Enter.
  //
  //   await alertDialog({title, message})                   → void
  //   await confirmDialog({title, message, danger?})        → boolean
  //   await promptDialog({title, message, placeholder?, defaultValue?, danger?})
  //                                                         → string | null
  //
  // Single instance — `openDialog` queues subsequent calls (rare but
  // possible if a callback in one dialog opens another). Mount once in
  // `+page.svelte`.
  import { writable } from 'svelte/store';

  type DialogKind = 'alert' | 'confirm' | 'choice' | 'prompt';

  export interface DialogOptions {
    title?: string;
    message: string;
    /** "提示" / "确认" / "继续" — overrides the kind-default. */
    okLabel?: string;
    /** Defaults to "取消"; ignored for `alert`. */
    cancelLabel?: string;
    /** Color the OK button red — for revert / discard / delete. */
    danger?: boolean;
    /** Prompt-only: input placeholder + initial value. */
    placeholder?: string;
    defaultValue?: string;
    /** choice-only: label for the secondary (middle) button. */
    secondaryLabel?: string;
  }

  /** Resolves to 'primary' (main OK), 'secondary' (middle button), or 'cancel'. */
  export type ChoiceResult = 'primary' | 'secondary' | 'cancel';

  interface PendingDialog {
    kind: DialogKind;
    opts: DialogOptions;
    resolve: (v: any) => void;
  }

  // Queue: only one dialog renders at a time; new openDialog() calls
  // wait their turn. Matches what stacked native dialogs do.
  const queue: PendingDialog[] = [];
  const current = writable<PendingDialog | null>(null);

  function pump(): void {
    if (!queue.length) {
      current.set(null);
      return;
    }
    current.set(queue[0]);
  }

  function open<T>(kind: DialogKind, opts: DialogOptions): Promise<T> {
    return new Promise<T>((resolve) => {
      queue.push({ kind, opts, resolve });
      if (queue.length === 1) pump();
    });
  }

  /** Themed alert. Resolves to void after the user clicks OK or hits Esc/Enter. */
  export function alertDialog(opts: DialogOptions): Promise<void> {
    return open<void>('alert', opts);
  }

  /** Themed confirm. Resolves to true (OK) or false (cancel/Esc). */
  export function confirmDialog(opts: DialogOptions): Promise<boolean> {
    return open<boolean>('confirm', opts);
  }

  /** Themed prompt. Resolves to entered string, or null on cancel/Esc. */
  export function promptDialog(opts: DialogOptions): Promise<string | null> {
    return open<string | null>('prompt', opts);
  }

  /** Three-button dialog. Resolves to 'primary', 'secondary', or 'cancel'.
   *  Use `secondaryLabel` in opts for the middle button text. */
  export function choiceDialog(opts: DialogOptions & { secondaryLabel: string }): Promise<ChoiceResult> {
    return open<ChoiceResult>('choice', opts);
  }

  // Internal — components script reads `currentDialog`.
  export const currentDialog = { subscribe: current.subscribe };

  /** Resolve the head-of-queue dialog and advance the queue. Internal —
   *  intentionally not exported so external callers can't double-resolve
   *  a pending promise (round-33 review MEDIUM). The component's
   *  instance script reaches it via lexical scope. */
  function _resolveCurrent(value: unknown): void {
    const head = queue.shift();
    if (head) head.resolve(value);
    pump();
  }
</script>

<script lang="ts">
  import { tick } from 'svelte';
  import { X, AlertTriangle } from 'lucide-svelte';
  import { tr } from '$lib/i18n';

  let dialog = $state<PendingDialog | null>(null);
  let inputValue = $state('');
  let inputEl: HTMLInputElement | undefined = $state();
  let okButtonEl: HTMLButtonElement | undefined = $state();

  // Subscribe directly so we can run side effects (focus trap, default
  // value seed) on every dialog change.
  currentDialog.subscribe((d) => {
    dialog = d;
    if (d?.kind === 'prompt') {
      inputValue = d.opts.defaultValue ?? '';
      void tick().then(() => inputEl?.focus());
    } else if (d) {
      void tick().then(() => okButtonEl?.focus());
    }
  });

  function onCancel(): void {
    if (!dialog) return;
    if (dialog.kind === 'alert') {
      _resolveCurrent(undefined);
    } else if (dialog.kind === 'confirm') {
      _resolveCurrent(false);
    } else if (dialog.kind === 'choice') {
      _resolveCurrent('cancel');
    } else {
      _resolveCurrent(null);
    }
  }

  /** Backdrop click: cancel only when the user hasn't typed anything
   *  into a prompt. Otherwise a stray click outside the dialog would
   *  silently discard their work. Esc / explicit Cancel still always
   *  dismiss (round-33 review LOW). */
  function onBackdropClick(): void {
    if (dialog?.kind === 'prompt' && inputValue.length > 0) return;
    onCancel();
  }

  function onOk(): void {
    if (!dialog) return;
    if (dialog.kind === 'alert') {
      _resolveCurrent(undefined);
    } else if (dialog.kind === 'confirm') {
      _resolveCurrent(true);
    } else if (dialog.kind === 'choice') {
      _resolveCurrent('primary');
    } else {
      _resolveCurrent(inputValue);
    }
  }

  function onSecondary(): void {
    if (!dialog || dialog.kind !== 'choice') return;
    _resolveCurrent('secondary');
  }

  function onKeydown(e: KeyboardEvent): void {
    // Defer to the IME during CJK / multibyte composition. Enter while
    // composing usually means "select this candidate", not "submit the
    // form" — every other keydown handler in the repo already carries
    // this guard (round-33 review HIGH).
    if (e.isComposing) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      onCancel();
    } else if (e.key === 'Enter') {
      // For prompts, only fire OK on Enter when the input is focused
      // (otherwise typing into the input would still need Enter to submit
      // even though Enter on the OK button does the same thing). For
      // confirm/alert, Enter anywhere triggers OK.
      e.preventDefault();
      onOk();
    }
  }

  // OK label resolution: explicit > kind-default
  function okLabel(): string {
    if (dialog?.opts.okLabel) return dialog.opts.okLabel;
    if (dialog?.kind === 'alert') return tr('ui.dialogOkAlert');
    if (dialog?.kind === 'confirm') return tr('ui.dialogOkConfirm');
    if (dialog?.kind === 'choice') return tr('ui.dialogOkChoice');
    return tr('ui.dialogOkPrompt');
  }
</script>

{#if dialog}
  <!-- z-index 9998 matches the modal registry; sits above launcher (9997)
       and history (9996), at parity with DiffEditorModal. ContextMenu
       (9999) still wins so a right-click menu spawned mid-dialog
       remains accessible. -->
  <div
    role="presentation"
    class="fixed inset-0 z-[9998] bg-black/55 flex items-center justify-center"
    onclick={onBackdropClick}
    onkeydown={onKeydown}
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-label={dialog.opts.title ?? tr('ui.dialogAriaLabel')}
      tabindex="-1"
      class="w-[min(420px,92vw)] flex flex-col bg-[var(--rg-bg-raised)] border border-[var(--rg-border)] rounded-lg shadow-2xl overflow-hidden"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <!-- Header — title + close (skipped for plain alert without title). -->
      {#if dialog.opts.title || dialog.kind !== 'alert'}
        <div class="flex items-center gap-2 px-3 h-9 border-b border-[var(--rg-border)] bg-[var(--rg-surface)]/60 shrink-0">
          {#if dialog.opts.danger}
            <AlertTriangle class="h-3.5 w-3.5 text-amber-400 shrink-0" />
          {/if}
          <span class="text-[12px] font-semibold flex-1 truncate">{dialog.opts.title ?? tr('ui.dialogDefaultTitle')}</span>
          <button
            type="button"
            class="flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors"
            title={tr('ui.dialogCloseTitle')}
            onclick={onCancel}
          >
            <X class="h-3.5 w-3.5" />
          </button>
        </div>
      {/if}

      <!-- Body — message + (for prompt) input. whitespace-pre-line so
           callers can use \n for hard breaks. -->
      <div class="px-4 py-3 flex flex-col gap-2">
        <div class="text-[12px] text-[var(--rg-fg)] whitespace-pre-line leading-relaxed">
          {dialog.opts.message}
        </div>
        {#if dialog.kind === 'prompt'}
          <input
            bind:this={inputEl}
            bind:value={inputValue}
            type="text"
            placeholder={dialog.opts.placeholder ?? ''}
            class="w-full px-2 py-1 text-[12px] rounded bg-[var(--rg-bg)] border border-[var(--rg-border)] text-[var(--rg-fg)] focus:outline-none focus:border-[var(--rg-accent)]/60"
          />
        {/if}
      </div>

      <!-- Footer — Cancel (left) + optional secondary + OK (right). For
           alert there is no cancel; the close button + Esc still dismiss. -->
      <div class="flex items-center justify-end gap-1.5 px-3 py-2 border-t border-[var(--rg-border)] bg-[var(--rg-surface)]/40">
        {#if dialog.kind !== 'alert'}
          <button
            type="button"
            class="px-2.5 py-1 rounded text-[11px] border border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors"
            onclick={onCancel}
          >
            {dialog.opts.cancelLabel ?? tr('ui.dialogCancel')}
          </button>
        {/if}
        {#if dialog.kind === 'choice' && dialog.opts.secondaryLabel}
          <button
            type="button"
            class="px-2.5 py-1 rounded text-[11px] border border-[var(--rg-border)] text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors"
            onclick={onSecondary}
          >
            {dialog.opts.secondaryLabel}
          </button>
        {/if}
        <button
          bind:this={okButtonEl}
          type="button"
          class="px-2.5 py-1 rounded text-[11px] border transition-colors {dialog.opts.danger
            ? 'border-rose-500/60 bg-rose-500/15 text-rose-200 hover:bg-rose-500/25'
            : 'border-[var(--rg-accent)]/60 bg-[var(--rg-accent)]/15 text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/25'}"
          onclick={onOk}
        >
          {okLabel()}
        </button>
      </div>
    </div>
  </div>
{/if}
