<script lang="ts">
  // Ridge Cloud — Pro 升级 / 登录 玻璃拟物 Modal（契约 §4.1/§4.2）。
  //
  // 付费方案完全由界面语言（locale）决定，互斥展示，不再手动切换地区：
  //   中文(zh)   → 仅「面包多卡密激活」（亮点页主推 + 卡密激活 tab）
  //   English(en) → 仅「Lemon Squeezy 信用卡订阅」（亮点页主推，外链）
  //   [ 本地登录 ] → 邮箱密码登录（/auth/login），两种语言均可用
  //
  // 设计：glassmorphism with real depth（design-quality），避免模板感。

  import { Zap, X, KeyRound, LogIn, ExternalLink, Loader2 } from 'lucide-svelte';
  import * as auth from './auth';
  import { cloudAuth } from './auth';
  import { ApiError } from './apiClient';
  import { t, tr, billingRegion } from '$lib/i18n';

  interface Props {
    open: boolean;
    onClose: () => void;
    /** 登录或激活成功后回调（携带最新登录态）。 */
    onReady: () => void;
  }

  let { open = $bindable(), onClose, onReady }: Props = $props();

  // 外链占位（运营后续替换为真实链接）。
  const LEMON_SQUEEZY_URL = 'https://ridge.lemonsqueezy.com/buy/PLACEHOLDER';
  const MBD_URL = 'https://mbd.pub/o/PLACEHOLDER';

  type Tab = 'highlights' | 'login' | 'activate';
  let tab = $state<Tab>('highlights');

  // 中文走面包多卡密；外文走海外订阅。完全由语言派生。
  const isCn = $derived($billingRegion === 'cn');

  // 切语言导致从「卡密激活」tab 变得不可用时，回落到亮点页。
  $effect(() => {
    if (!isCn && tab === 'activate') tab = 'highlights';
  });

  // 登录表单
  let email = $state('');
  let password = $state('');
  // 卡密
  let licenseKey = $state('');
  let activateUsername = $state('');

  let busy = $state(false);
  let errorMsg = $state('');

  function handleError(e: unknown): void {
    if (e instanceof ApiError) {
      const msg = tr(`errors.${e.code}`);
      errorMsg = msg === `errors.${e.code}` ? tr('errors.GENERIC') : msg;
    } else {
      errorMsg = tr('errors.GENERIC');
    }
  }

  async function doLogin(): Promise<void> {
    errorMsg = '';
    busy = true;
    try {
      await auth.login(email.trim(), password);
      onReady();
      close();
    } catch (e) {
      handleError(e);
    } finally {
      busy = false;
    }
  }

  async function doActivate(): Promise<void> {
    errorMsg = '';
    busy = true;
    try {
      await auth.activateKey(licenseKey.trim().toUpperCase(), activateUsername.trim() || undefined);
      onReady();
      close();
    } catch (e) {
      handleError(e);
    } finally {
      busy = false;
    }
  }

  function openExternal(url: string): void {
    // Tauri opener 优先；不可用时退回 window.open。
    import('@tauri-apps/plugin-opener')
      .then((m) => m.openUrl(url))
      .catch(() => { window.open(url, '_blank', 'noopener'); });
  }

  function close(): void {
    errorMsg = '';
    onClose();
  }

  const loggedIn = $derived(auth.isLoggedIn($cloudAuth));

  // 可见 tab 列表随语言变化：外文隐藏「卡密激活」。
  const tabs = $derived(
    isCn
      ? ([['highlights', $t('cloudPro.tabHighlights')], ['login', $t('cloudPro.tabLogin')], ['activate', $t('cloudPro.tabActivate')]] as const)
      : ([['highlights', $t('cloudPro.tabHighlights')], ['login', $t('cloudPro.tabLogin')]] as const)
  );
</script>

{#if open}
  <!-- backdrop -->
  <div
    class="fixed inset-0 z-[200] flex items-center justify-center p-4"
    style="background: color-mix(in oklch, var(--rg-bg) 55%, transparent); backdrop-filter: blur(6px);"
    role="presentation"
    onclick={(e) => { if (e.target === e.currentTarget) close(); }}
  >
    <!-- glass card -->
    <div
      class="relative w-full max-w-md overflow-hidden rounded-2xl border shadow-2xl"
      style="
        border-color: color-mix(in oklch, var(--rg-accent) 30%, var(--rg-border));
        background:
          linear-gradient(155deg,
            color-mix(in oklch, var(--rg-bg-raised, var(--rg-surface)) 88%, transparent) 0%,
            color-mix(in oklch, var(--rg-surface) 72%, transparent) 100%);
        backdrop-filter: blur(28px) saturate(140%);
        box-shadow: 0 24px 64px -16px rgba(0,0,0,0.55), inset 0 1px 0 0 color-mix(in oklch, white 14%, transparent);
      "
      role="dialog"
      aria-modal="true"
      aria-labelledby="cloud-pro-title"
    >
      <!-- accent glow ribbon -->
      <div
        class="pointer-events-none absolute -top-24 left-1/2 h-48 w-48 -translate-x-1/2 rounded-full opacity-40 blur-3xl"
        style="background: radial-gradient(circle, var(--rg-accent) 0%, transparent 70%);"
      ></div>

      <!-- close -->
      <button
        onclick={close}
        class="absolute right-3 top-3 z-10 grid h-7 w-7 place-items-center rounded-lg text-[var(--rg-fg-muted)] transition-colors hover:bg-white/10 hover:text-[var(--rg-fg)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/60"
        aria-label={$t('cloudPro.closeLabel')}
      >
        <X class="h-4 w-4" />
      </button>

      <div class="relative px-6 pb-6 pt-7">
        <!-- header -->
        <div class="mb-5 flex items-center gap-3">
          <div
            class="grid h-11 w-11 shrink-0 place-items-center rounded-xl"
            style="background: color-mix(in oklch, var(--rg-accent) 18%, transparent); box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--rg-accent) 40%, transparent);"
          >
            <Zap class="h-5 w-5 text-[var(--rg-accent)]" />
          </div>
          <div class="min-w-0">
            <h2 id="cloud-pro-title" class="text-lg font-semibold leading-tight text-[var(--rg-fg)]">
              {$t('cloudPro.title')}
            </h2>
            <p class="text-xs text-[var(--rg-fg-muted)]">{$t('cloudPro.subtitle')}</p>
          </div>
        </div>

        <!-- tabs -->
        <div class="mb-5 flex gap-1 rounded-lg bg-black/20 p-1 text-xs">
          {#each tabs as [key, label] (key)}
            <button
              onclick={() => { tab = key as Tab; errorMsg = ''; }}
              class="flex-1 rounded-md px-2 py-1.5 font-medium transition-all duration-150 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50
                {tab === key
                  ? 'bg-[var(--rg-accent)]/20 text-[var(--rg-fg)] shadow-sm'
                  : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
            >
              {label}
            </button>
          {/each}
        </div>

        {#if tab === 'highlights'}
          <ul class="mb-5 space-y-3">
            {#each [
              [$t('cloudPro.feat1Title'), $t('cloudPro.feat1Desc')],
              [$t('cloudPro.feat2Title'), $t('cloudPro.feat2Desc')],
              [$t('cloudPro.feat3Title'), $t('cloudPro.feat3Desc')],
            ] as [title, desc] (title)}
              <li class="flex gap-3">
                <span class="mt-0.5 grid h-5 w-5 shrink-0 place-items-center rounded-full text-[var(--rg-accent)]" style="background: color-mix(in oklch, var(--rg-accent) 16%, transparent);">
                  <Zap class="h-3 w-3" />
                </span>
                <div>
                  <p class="text-sm font-medium text-[var(--rg-fg)]">{title}</p>
                  <p class="text-xs leading-relaxed text-[var(--rg-fg-muted)]">{desc}</p>
                </div>
              </li>
            {/each}
          </ul>

          <!-- 付费方案由语言决定，互斥展示 -->
          <div class="space-y-2">
            {#if isCn}
              <!-- 中文：面包多卡密 -->
              <button
                onclick={() => { tab = 'activate'; }}
                class="group flex w-full items-center justify-center gap-2 rounded-xl py-2.5 text-sm font-semibold text-white transition-all duration-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/60"
                style="background: linear-gradient(135deg, var(--rg-accent) 0%, color-mix(in oklch, var(--rg-accent) 70%, #7c3aed) 100%); box-shadow: 0 8px 24px -8px var(--rg-accent);"
              >
                <KeyRound class="h-4 w-4" /> {$t('cloudPro.cnPrimary')}
              </button>
              <button
                onclick={() => { tab = 'login'; }}
                class="w-full rounded-xl border border-[var(--rg-border)] py-2 text-xs font-medium text-[var(--rg-fg)] transition-colors hover:border-[var(--rg-accent)]/40 hover:bg-white/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50"
              >
                {$t('cloudPro.tabLogin')}
              </button>
            {:else}
              <!-- 外文：Lemon Squeezy 信用卡 -->
              <button
                onclick={() => openExternal(LEMON_SQUEEZY_URL)}
                class="group flex w-full items-center justify-center gap-2 rounded-xl py-2.5 text-sm font-semibold text-white transition-all duration-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/60"
                style="background: linear-gradient(135deg, var(--rg-accent) 0%, color-mix(in oklch, var(--rg-accent) 70%, #7c3aed) 100%); box-shadow: 0 8px 24px -8px var(--rg-accent);"
              >
                <Zap class="h-4 w-4" /> {$t('cloudPro.intlPrimary')}
                <ExternalLink class="h-3.5 w-3.5 opacity-70 transition-transform group-hover:translate-x-0.5" />
              </button>
              <p class="px-1 text-[11px] leading-relaxed text-[var(--rg-fg-muted)]">{$t('cloudPro.intlHint')}</p>
              <button
                onclick={() => { tab = 'login'; }}
                class="w-full rounded-xl border border-[var(--rg-border)] py-2 text-xs font-medium text-[var(--rg-fg)] transition-colors hover:border-[var(--rg-accent)]/40 hover:bg-white/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50"
              >
                {$t('cloudPro.tabLogin')}
              </button>
            {/if}
          </div>
        {:else if tab === 'login'}
          <form class="space-y-3" onsubmit={(e) => { e.preventDefault(); void doLogin(); }}>
            <label class="block">
              <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">{$t('cloudPro.loginEmail')}</span>
              <input
                bind:value={email}
                type="email"
                autocomplete="email"
                required
                class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 text-sm text-[var(--rg-fg)] outline-none transition-colors placeholder:text-[var(--rg-fg-muted)]/60 focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
                placeholder="you@example.com"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">{$t('cloudPro.loginPassword')}</span>
              <input
                bind:value={password}
                type="password"
                autocomplete="current-password"
                required
                class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 text-sm text-[var(--rg-fg)] outline-none transition-colors focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
                placeholder="••••••••"
              />
            </label>
            <button
              type="submit"
              disabled={busy}
              class="flex w-full items-center justify-center gap-2 rounded-xl bg-[var(--rg-accent)] py-2.5 text-sm font-semibold text-white transition-all hover:brightness-110 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/60"
            >
              {#if busy}<Loader2 class="h-4 w-4 animate-spin" />{:else}<LogIn class="h-4 w-4" />{/if}
              {$t('cloudPro.loginSubmit')}
            </button>
          </form>
        {:else}
          <form class="space-y-3" onsubmit={(e) => { e.preventDefault(); void doActivate(); }}>
            <p class="text-xs leading-relaxed text-[var(--rg-fg-muted)]">
              {$t('cloudPro.activateBuyHint')}
              <button type="button" onclick={() => openExternal(MBD_URL)} class="text-[var(--rg-accent)] hover:underline">
                {$t('cloudPro.cnGoMbd')}
              </button>
            </p>
            {#if !loggedIn}
              <p class="rounded-lg bg-amber-500/10 px-3 py-2 text-xs text-amber-400">
                {$t('cloudPro.activateNeedLogin')}
              </p>
            {/if}
            <label class="block">
              <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">{$t('cloudPro.licenseKey')}</span>
              <input
                bind:value={licenseKey}
                required
                class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 font-mono text-sm uppercase tracking-wider text-[var(--rg-fg)] outline-none transition-colors focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
                placeholder="RIDGE-XXXX-XXXX-XXXX"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">{$t('cloudPro.activateUsername')}</span>
              <input
                bind:value={activateUsername}
                class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 text-sm text-[var(--rg-fg)] outline-none transition-colors focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
                placeholder={$t('cloudPro.activateUsernamePlaceholder')}
              />
            </label>
            <button
              type="submit"
              disabled={busy || !loggedIn}
              class="flex w-full items-center justify-center gap-2 rounded-xl bg-[var(--rg-accent)] py-2.5 text-sm font-semibold text-white transition-all hover:brightness-110 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/60"
            >
              {#if busy}<Loader2 class="h-4 w-4 animate-spin" />{:else}<KeyRound class="h-4 w-4" />{/if}
              {$t('cloudPro.activateSubmit')}
            </button>
          </form>
        {/if}

        {#if errorMsg}
          <p class="mt-3 text-center text-xs text-red-400">{errorMsg}</p>
        {/if}
      </div>
    </div>
  </div>
{/if}
