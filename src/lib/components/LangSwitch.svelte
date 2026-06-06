<script lang="ts">
  // 语言切换器：中文 / English 二档分段控件。
  // 切换语言同时驱动付费方案展示（zh→爱发电卡密，en→海外信用卡订阅）。
  import { locale, setLocale, t, LOCALES, type Locale } from '$lib/i18n';

  const fullLabel: Record<Locale, string> = { zh: '中文', en: 'English' };
</script>

<div
  class="inline-flex items-center gap-0.5 rounded-lg bg-[var(--rg-surface)]/60 p-1 text-xs"
  role="group"
  aria-label={$t('lang.label')}
>
  {#each LOCALES as l (l)}
    <button
      type="button"
      onclick={() => setLocale(l)}
      aria-pressed={$locale === l}
      title={l === 'zh' ? $t('lang.switchToZh') : $t('lang.switchToEn')}
      class="rounded-md px-3 py-1.5 font-medium transition-all duration-150 outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50
        {$locale === l
          ? 'bg-[var(--rg-accent)]/20 text-[var(--rg-fg)] shadow-sm'
          : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
    >
      {fullLabel[l]}
    </button>
  {/each}
</div>
