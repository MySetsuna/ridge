<script lang="ts">
  // 可复用开关：rem 自洽几何，圆钮用 left(% + rem) 定位，数学上不会溢出轨道。
  interface Props {
    checked: boolean;
    onchange: (next: boolean) => void;
    disabled?: boolean;
    ariaLabel?: string;
    title?: string;
  }
  let { checked, onchange, disabled = false, ariaLabel, title }: Props = $props();
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={ariaLabel}
  {title}
  {disabled}
  onclick={() => onchange(!checked)}
  class="relative inline-flex shrink-0 h-5 w-9 rounded-full border transition-colors disabled:opacity-50 disabled:cursor-not-allowed {checked
    ? 'bg-[var(--rg-accent)] border-[var(--rg-accent)]'
    : 'bg-[var(--rg-surface-2)] border-[var(--rg-border)]'}"
>
  <span
    class="pointer-events-none absolute top-1/2 -translate-y-1/2 h-4 w-4 rounded-full bg-white shadow-sm transition-[left] duration-150 {checked
      ? 'left-[calc(100%_-_1.125rem)]'
      : 'left-0.5'}"
  ></span>
</button>
