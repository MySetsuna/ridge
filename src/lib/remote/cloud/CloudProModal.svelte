<script lang="ts">
  // Ridge Cloud — Pro 升级 / 登录 玻璃拟物 Modal（契约 §4.1/§4.2）。
  //
  // 付费方案完全由界面语言（locale）决定，互斥展示，不再手动切换地区：
  //   中文(zh)   → 仅「爱发电订阅 / 卡密激活」（亮点页主推 + 卡密激活 tab）
  //   English(en) → 仅「Lemon Squeezy 信用卡订阅」（亮点页主推，外链）
  //   [ 本地登录 ] → 邮箱密码登录（/auth/login），两种语言均可用
  //
  // 设计：glassmorphism with real depth（design-quality），避免模板感。

  import { Zap, X, KeyRound, LogIn, ExternalLink, Loader2, Globe, CalendarCheck, Mail, ArrowLeft } from 'lucide-svelte';
  import { portal } from '$lib/actions/portal';
  import * as auth from './auth';
  import { cloudAuth } from './auth';
  import { ApiError, BASE_DOMAIN } from './apiClient';
  import { t, tr, locale, billingRegion } from '$lib/i18n';

  interface Props {
    open: boolean;
    onClose: () => void;
    /** 登录或激活成功后回调（携带最新登录态）。 */
    onReady: () => void;
  }

  let { open = $bindable(), onClose, onReady }: Props = $props();

  // 爱发电（zh，契约 §7 真实链接）；海外订阅 Lemon Squeezy 仍为占位待运营填。
  const LEMON_SQUEEZY_URL = 'https://ridge.lemonsqueezy.com/buy/PLACEHOLDER';
  const AFDIAN_URL = 'https://ifdian.net/a/ridge';

  type Tab = 'highlights' | 'login' | 'activate';
  let tab = $state<Tab>('highlights');

  // 中文走爱发电订阅/卡密；外文走海外订阅。完全由语言派生。
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

  // 浏览器登录授权（§2.3）：主登录方式；邮箱密码作为兜底（默认折叠）。
  let browserBusy = $state(false);
  let showLocalLogin = $state(false);
  let authorizeUrl = $state('');
  let browserAbort: AbortController | null = null;

  let busy = $state(false);
  let errorMsg = $state('');

  // 忘记密码流程（login tab 内嵌）
  let forgotStep = $state<'idle' | 'email' | 'code'>('idle');
  let forgotEmail = $state('');
  let forgotCode = $state('');
  let forgotNewPassword = $state('');
  let forgotConfirmPassword = $state('');
  let forgotBusy = $state(false);

  async function doForgotPassword(): Promise<void> {
    errorMsg = '';
    forgotBusy = true;
    try {
      await auth.forgotPassword(forgotEmail.trim());
      forgotStep = 'code';
    } catch (e) {
      handleError(e);
    } finally {
      forgotBusy = false;
    }
  }

  async function doResetPassword(): Promise<void> {
    errorMsg = '';
    if (forgotNewPassword !== forgotConfirmPassword) {
      errorMsg = $t('cloudPro.forgotPasswordMismatch');
      return;
    }
    if (forgotNewPassword.length < 8) {
      errorMsg = $t('cloudPro.forgotPasswordTooShort');
      return;
    }
    forgotBusy = true;
    try {
      await auth.resetPassword(forgotEmail.trim(), forgotCode.trim(), forgotNewPassword);
      onReady();
      close();
    } catch (e) {
      handleError(e);
    } finally {
      forgotBusy = false;
    }
  }

  // §5 每日签到：free 用户每日得 2h 免费公网远控。成功/已签到/永久 premium 各有反馈。
  let checkinBusy = $state(false);
  let checkinMsg = $state('');

  /** 把签到到期秒级 unix 格式化为本地化「至 HH:mm」展示。 */
  function formatGrantedUntil(expiresAt: number | null): string {
    if (expiresAt == null) return '';
    const ms = expiresAt < 1e12 ? expiresAt * 1000 : expiresAt;
    const intlLocale = $locale === 'zh' ? 'zh-CN' : 'en-US';
    return new Date(ms).toLocaleString(intlLocale, {
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  async function doCheckin(): Promise<void> {
    if (checkinBusy) return;
    errorMsg = '';
    checkinMsg = '';
    checkinBusy = true;
    try {
      const res = await auth.checkin();
      if (res.ok) {
        // 成功授予 2h 临时 premium → 展示授予截止时间；onReady 让上层据新 premium 态联动。
        checkinMsg = tr('cloudPro.checkinGranted', { until: formatGrantedUntil(res.premiumExpiresAt) });
        onReady();
      } else if (res.reason === 'permanent') {
        // 已是永久/买断 premium：签到入口本应隐藏，兜底联动一次。
        checkinMsg = '';
        onReady();
      } else {
        // reason==='already'：今日已签到。
        checkinMsg = tr('cloudPro.checkinAlready');
      }
    } catch (e) {
      handleError(e);
    } finally {
      checkinBusy = false;
    }
  }

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

  // §2.3 浏览器登录授权：打开默认浏览器 → 轮询拿 user JWT。`ridge://auth/focus`
  // 唤起桌面端后由 Rust 广播 `ridge://auth-focus`，桥接为 onWake 立即触发一次轮询。
  async function doBrowserLogin(): Promise<void> {
    if (browserBusy) return;
    errorMsg = '';
    authorizeUrl = '';
    browserBusy = true;
    browserAbort = new AbortController();
    try {
      await auth.loginViaBrowser({
        signal: browserAbort.signal,
        onProgress: (p) => { authorizeUrl = p.authorizeUrl; },
        onWake: (cb) => listenAuthFocus(cb),
      });
      onReady();
      close();
    } catch (e) {
      // 用户主动取消（关弹窗）不报错。
      if (e instanceof ApiError && e.code === 'INVALID_INPUT') return;
      handleError(e);
    } finally {
      browserBusy = false;
      browserAbort = null;
      authorizeUrl = '';
    }
  }

  // 把 Tauri `ridge://auth-focus` 事件桥接为「立即再轮询」唤醒。纯浏览器
  // （web-remote shim）下 listen 不可用时静默退化为按 interval 轮询。
  function listenAuthFocus(cb: () => void): () => void {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    import('@tauri-apps/api/event')
      .then((m) => m.listen('ridge://auth-focus', () => cb()))
      .then((un) => { if (cancelled) un(); else unlisten = un; })
      .catch(() => { /* 非 Tauri 环境，退化为定时轮询 */ });
    return () => { cancelled = true; unlisten?.(); };
  }

  async function doActivate(): Promise<void> {
    errorMsg = '';
    busy = true;
    try {
      // 用户名一律取自登录态（不再让用户在激活时重复填写）。
      await auth.activateKey(licenseKey.trim().toUpperCase());
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
    forgotStep = 'idle';
    forgotEmail = '';
    forgotCode = '';
    forgotNewPassword = '';
    forgotConfirmPassword = '';
    // 关弹窗即取消进行中的浏览器登录轮询（loginViaBrowser 会抛 INVALID_INPUT，已忽略）。
    browserAbort?.abort();
    onClose();
  }

  const loggedIn = $derived(auth.isLoggedIn($cloudAuth));
  // premium 已激活（按缓存 plan）。已激活则不展示任何升级/签到入口（契约 §5/§7）。
  const premiumActive = $derived(auth.isPremium($cloudAuth));
  // 签到入口：已登录且非 premium 才展示（free 路径）。
  const showCheckin = $derived(loggedIn && !premiumActive);

  // 可见 tab 列表随语言变化：外文隐藏「卡密激活」。
  const tabs = $derived(
    isCn
      ? ([['highlights', $t('cloudPro.tabHighlights')], ['login', $t('cloudPro.tabLogin')], ['activate', $t('cloudPro.tabActivate')]] as const)
      : ([['highlights', $t('cloudPro.tabHighlights')], ['login', $t('cloudPro.tabLogin')]] as const)
  );
</script>

{#if open}
  <!-- backdrop — `use:portal` 把整个遮罩移到 <body>，逃出远控侧边栏的
       backdrop-filter 包含块（否则 position:fixed 会以侧边栏而非整窗为参照系，
       弹窗被困在侧栏内）。与 SettingsPanel 一样以整个客户端为参照居中。 -->
  <div
    use:portal={{ id: 'cloud-pro-modal' }}
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
              [$t('cloudPro.feat2Title'), $t('cloudPro.feat2Desc', { base: BASE_DOMAIN })],
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
              <!-- 中文：爱发电订阅 / 卡密 -->
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

          <!-- §5 每日签到（free 路径，与上方订阅/卡密升级入口并存）：已登录且非 premium 才展示。 -->
          {#if showCheckin}
            <div class="mt-4 rounded-xl border border-dashed border-[var(--rg-border)] p-3">
              <p class="mb-2 text-[11px] leading-relaxed text-[var(--rg-fg-muted)]">{$t('cloudPro.checkinHint')}</p>
              <button
                type="button"
                onclick={() => void doCheckin()}
                disabled={checkinBusy}
                class="flex w-full items-center justify-center gap-2 rounded-xl border border-[var(--rg-accent)]/40 py-2 text-sm font-semibold text-[var(--rg-accent)] transition-all hover:bg-[var(--rg-accent)]/10 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50"
              >
                {#if checkinBusy}<Loader2 class="h-4 w-4 animate-spin" />{:else}<CalendarCheck class="h-4 w-4" />{/if}
                {$t('cloudPro.checkinBtn')}
              </button>
              {#if checkinMsg}
                <p class="mt-2 text-center text-[11px] text-green-400">{checkinMsg}</p>
              {/if}
            </div>
          {/if}
        {:else if tab === 'login'}
          {#if forgotStep === 'idle'}
            <!-- §2.3 主登录：浏览器授权（类似 Claude Code 登录）。 -->
            <div class="space-y-3">
              <button
                type="button"
                onclick={() => void doBrowserLogin()}
                disabled={browserBusy}
                class="flex w-full items-center justify-center gap-2 rounded-xl bg-[var(--rg-accent)] py-2.5 text-sm font-semibold text-white transition-all hover:brightness-110 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/60"
              >
                {#if browserBusy}<Loader2 class="h-4 w-4 animate-spin" />{:else}<Globe class="h-4 w-4" />{/if}
                {$t('cloudPro.loginBrowser')}
              </button>
              {#if browserBusy}
                <p class="text-center text-[11px] text-[var(--rg-fg-muted)]">{$t('cloudPro.loginBrowserWaiting')}</p>
                {#if authorizeUrl}
                  <button
                    type="button"
                    onclick={() => openExternal(authorizeUrl)}
                    class="block w-full text-center text-[11px] text-[var(--rg-accent)] hover:underline"
                  >
                    {$t('cloudPro.loginBrowserFallback')}
                  </button>
                {/if}
              {:else}
                <p class="px-1 text-center text-[11px] leading-relaxed text-[var(--rg-fg-muted)]">{$t('cloudPro.loginBrowserHint')}</p>
              {/if}

              <!-- 兜底：邮箱密码登录（默认折叠）。 -->
              <button
                type="button"
                onclick={() => { showLocalLogin = !showLocalLogin; errorMsg = ''; }}
                class="w-full text-center text-[11px] text-[var(--rg-fg-muted)] underline-offset-2 hover:text-[var(--rg-fg)] hover:underline"
              >
                {$t('cloudPro.loginLocalToggle')}
              </button>
              {#if showLocalLogin}
                <form class="space-y-3 border-t border-[var(--rg-border)]/60 pt-3" onsubmit={(e) => { e.preventDefault(); void doLogin(); }}>
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
                    class="flex w-full items-center justify-center gap-2 rounded-xl border border-[var(--rg-border)] py-2 text-sm font-medium text-[var(--rg-fg)] transition-all hover:border-[var(--rg-accent)]/40 hover:bg-white/5 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50"
                  >
                    {#if busy}<Loader2 class="h-4 w-4 animate-spin" />{:else}<LogIn class="h-4 w-4" />{/if}
                    {$t('cloudPro.loginSubmit')}
                  </button>
                  <button
                    type="button"
                    onclick={() => { forgotStep = 'email'; forgotEmail = email || forgotEmail; errorMsg = ''; }}
                    class="block w-full text-center text-[11px] text-[var(--rg-accent)] hover:underline"
                  >
                    {$t('cloudPro.forgotPasswordLink')}
                  </button>
                </form>
              {/if}
            </div>
          {:else if forgotStep === 'email'}
            <!-- 忘记密码：输入邮箱发送重置码 -->
            <div class="space-y-3">
              <button
                type="button"
                onclick={() => { forgotStep = 'idle'; errorMsg = ''; }}
                class="flex items-center gap-1 text-xs text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
              >
                <ArrowLeft class="h-3.5 w-3.5" />
                {$t('cloudPro.forgotPasswordBack')}
              </button>
              <p class="text-xs leading-relaxed text-[var(--rg-fg-muted)]">{$t('cloudPro.forgotPasswordSentHint')}</p>
              <label class="block">
                <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">{$t('cloudPro.forgotPasswordEmail')}</span>
                <input
                  bind:value={forgotEmail}
                  type="email"
                  autocomplete="email"
                  required
                  class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 text-sm text-[var(--rg-fg)] outline-none transition-colors placeholder:text-[var(--rg-fg-muted)]/60 focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
                  placeholder="you@example.com"
                />
              </label>
              <button
                type="button"
                onclick={() => void doForgotPassword()}
                disabled={forgotBusy}
                class="flex w-full items-center justify-center gap-2 rounded-xl border border-[var(--rg-border)] py-2 text-xs font-medium text-[var(--rg-fg)] transition-colors hover:border-[var(--rg-accent)]/40 hover:bg-white/5 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50"
              >
                {#if forgotBusy}<Loader2 class="h-3.5 w-3.5 animate-spin" />{:else}<Mail class="h-3.5 w-3.5" />{/if}
                {$t('cloudPro.forgotPasswordSend')}
              </button>
            </div>
          {:else if forgotStep === 'code'}
            <!-- 忘记密码：验证码 + 新密码表单 -->
            <div class="space-y-3">
              <button
                type="button"
                onclick={() => { forgotStep = 'idle'; errorMsg = ''; }}
                class="flex items-center gap-1 text-xs text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
              >
                <ArrowLeft class="h-3.5 w-3.5" />
                {$t('cloudPro.forgotPasswordBack')}
              </button>
              <form class="space-y-3" onsubmit={(e) => { e.preventDefault(); void doResetPassword(); }}>
                <label class="block">
                  <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">{$t('cloudPro.forgotPasswordEmail')}</span>
                  <input
                    bind:value={forgotEmail}
                    type="email"
                    autocomplete="email"
                    required
                    class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 text-sm text-[var(--rg-fg)] outline-none transition-colors focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
                  />
                </label>
                <label class="block">
                  <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">{$t('cloudPro.forgotPasswordCode')}</span>
                  <input
                    bind:value={forgotCode}
                    type="text"
                    inputmode="numeric"
                    pattern="[0-9]{6}"
                    maxlength="6"
                    autocomplete="one-time-code"
                    required
                    class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 text-center font-mono text-sm tracking-widest text-[var(--rg-fg)] outline-none transition-colors focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
                    placeholder="000000"
                  />
                </label>
                <label class="block">
                  <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">{$t('cloudPro.forgotPasswordNewPassword')}</span>
                  <input
                    bind:value={forgotNewPassword}
                    type="password"
                    autocomplete="new-password"
                    minlength="8"
                    required
                    class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 text-sm text-[var(--rg-fg)] outline-none transition-colors focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
                    placeholder="••••••••"
                  />
                </label>
                <label class="block">
                  <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">{$t('cloudPro.forgotPasswordConfirmPassword')}</span>
                  <input
                    bind:value={forgotConfirmPassword}
                    type="password"
                    autocomplete="new-password"
                    minlength="8"
                    required
                    class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 text-sm text-[var(--rg-fg)] outline-none transition-colors focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
                    placeholder="••••••••"
                  />
                </label>
                <button
                  type="submit"
                  disabled={forgotBusy}
                  class="flex w-full items-center justify-center gap-2 rounded-xl bg-[var(--rg-accent)] py-2.5 text-sm font-semibold text-white transition-all hover:brightness-110 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/60"
                >
                  {#if forgotBusy}<Loader2 class="h-4 w-4 animate-spin" />{:else}<KeyRound class="h-4 w-4" />{/if}
                  {$t('cloudPro.forgotPasswordSubmit')}
                </button>
                <button
                  type="button"
                  onclick={() => void doForgotPassword()}
                  disabled={forgotBusy}
                  class="block w-full text-center text-[11px] text-[var(--rg-accent)] hover:underline disabled:opacity-50"
                >
                  {$t('cloudPro.forgotPasswordResend')}
                </button>
              </form>
            </div>
          {/if}
        {:else}
          <form class="space-y-3" onsubmit={(e) => { e.preventDefault(); void doActivate(); }}>
            <p class="text-xs leading-relaxed text-[var(--rg-fg-muted)]">
              {$t('cloudPro.activateBuyHint')}
              <button type="button" onclick={() => openExternal(AFDIAN_URL)} class="text-[var(--rg-accent)] hover:underline">
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
