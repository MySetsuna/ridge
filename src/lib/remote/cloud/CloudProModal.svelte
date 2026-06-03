<script lang="ts">
  // Ridge Cloud — Pro 升级 / 登录 玻璃拟物 Modal（契约 §4.1/§4.2）。
  //
  // 结算方式按地区自动选默认（方案 1，detectPreferredRegion），可手动切换：
  //   国内 → [ 国内卡密激活 ] 主推（面包多购买 → /auth/activate-key）
  //   海外 → [ 立即订阅 ]    主推（外链 Lemon Squeezy）
  //   [ 本地登录 ] → 邮箱密码登录（/auth/login），始终可用
  //
  // 设计：glassmorphism with real depth（design-quality），避免模板感：
  //   层次（标题 scale 对比）、blur+边缘高光、hover/focus 态、语义化色彩。

  import { Zap, X, KeyRound, LogIn, ExternalLink, Loader2 } from 'lucide-svelte';
  import * as auth from './auth';
  import { cloudAuth } from './auth';
  import { ApiError } from './apiClient';
  import { detectPreferredRegion, type Region } from './region';

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

  // 方案 1：按语言/时区自动选默认结算地区，用户可随时切换。
  const recommended: Region = detectPreferredRegion();
  let region = $state<Region>(recommended);

  // 登录表单
  let email = $state('');
  let password = $state('');
  // 卡密
  let licenseKey = $state('');
  let activateUsername = $state('');

  let busy = $state(false);
  let errorMsg = $state('');

  function codeToMessage(code: string): string {
    const map: Record<string, string> = {
      UNAUTHORIZED: '账号或密码错误',
      FORBIDDEN: '没有权限',
      NOT_FOUND: '账号不存在',
      INVALID_INPUT: '输入有误，请检查',
      INVALID_KEY: '卡密无效',
      KEY_ALREADY_USED: '卡密已被使用',
      USERNAME_TAKEN: '该用户名已被占用',
      USERNAME_REQUIRED: '请先设置用户名',
      NOT_PREMIUM: '该账号尚未开通 Pro',
      DEVICE_NAME_TAKEN: '设备名已存在',
      RATE_LIMITED: '操作过于频繁，请稍后再试',
      NETWORK: '网络连接失败，请检查网络',
      BAD_RESPONSE: '服务器响应异常',
      INTERNAL: '服务器内部错误',
    };
    return map[code] ?? '操作失败，请重试';
  }

  function handleError(e: unknown): void {
    errorMsg = e instanceof ApiError ? codeToMessage(e.code) : '操作失败，请重试';
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
        aria-label="关闭"
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
              官方公网加速 · Pro
            </h2>
            <p class="text-xs text-[var(--rg-fg-muted)]">随时随地，安全直连你的设备</p>
          </div>
        </div>

        <!-- tabs -->
        <div class="mb-5 flex gap-1 rounded-lg bg-black/20 p-1 text-xs">
          {#each [['highlights', 'Pro 亮点'], ['login', '本地登录'], ['activate', '卡密激活']] as [key, label] (key)}
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
              ['一键穿透', '无需公网 IP、无需端口转发，自动 NAT 穿透直连'],
              ['专属二级域名', '{设备}-{用户名}.remo2ridge.duckdns.org，记得住、分享方便'],
              ['端到端加密', 'X25519 + ChaCha20-Poly1305，中继与 TURN 都看不到明文'],
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

          <!-- 结算方式切换：默认按地区自动选中，可手动切换 -->
          <div class="mb-1.5 flex gap-1 rounded-lg bg-black/20 p-1 text-xs" role="tablist" aria-label="结算方式">
            {#each [['cn', '🇨🇳 国内 · 卡密'], ['intl', '🌐 海外 · 信用卡']] as [key, label] (key)}
              <button
                role="tab"
                aria-selected={region === key}
                onclick={() => { region = key as Region; }}
                class="flex-1 rounded-md px-2 py-1.5 font-medium transition-all duration-150 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50
                  {region === key
                    ? 'bg-[var(--rg-accent)]/20 text-[var(--rg-fg)] shadow-sm'
                    : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
              >
                {label}{#if recommended === key}<span class="ml-1 text-[10px] text-[var(--rg-accent)]">推荐</span>{/if}
              </button>
            {/each}
          </div>
          <p class="mb-4 text-[11px] text-[var(--rg-fg-muted)]">已按你的语言与时区自动选择，可随时切换。</p>

          <div class="space-y-2">
            {#if region === 'cn'}
              <!-- 国内主推：面包多卡密 -->
              <button
                onclick={() => { tab = 'activate'; }}
                class="group flex w-full items-center justify-center gap-2 rounded-xl py-2.5 text-sm font-semibold text-white transition-all duration-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/60"
                style="background: linear-gradient(135deg, var(--rg-accent) 0%, color-mix(in oklch, var(--rg-accent) 70%, #7c3aed) 100%); box-shadow: 0 8px 24px -8px var(--rg-accent);"
              >
                <KeyRound class="h-4 w-4" /> 国内卡密激活
              </button>
              <div class="flex gap-2">
                <button
                  onclick={() => openExternal(LEMON_SQUEEZY_URL)}
                  class="flex-1 rounded-xl border border-[var(--rg-border)] py-2 text-xs font-medium text-[var(--rg-fg)] transition-colors hover:border-[var(--rg-accent)]/40 hover:bg-white/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50"
                >
                  海外信用卡订阅
                </button>
                <button
                  onclick={() => { tab = 'login'; }}
                  class="flex-1 rounded-xl border border-[var(--rg-border)] py-2 text-xs font-medium text-[var(--rg-fg)] transition-colors hover:border-[var(--rg-accent)]/40 hover:bg-white/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50"
                >
                  本地登录
                </button>
              </div>
            {:else}
              <!-- 海外主推：Lemon Squeezy 信用卡 -->
              <button
                onclick={() => openExternal(LEMON_SQUEEZY_URL)}
                class="group flex w-full items-center justify-center gap-2 rounded-xl py-2.5 text-sm font-semibold text-white transition-all duration-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/60"
                style="background: linear-gradient(135deg, var(--rg-accent) 0%, color-mix(in oklch, var(--rg-accent) 70%, #7c3aed) 100%); box-shadow: 0 8px 24px -8px var(--rg-accent);"
              >
                <Zap class="h-4 w-4" /> 立即订阅
                <ExternalLink class="h-3.5 w-3.5 opacity-70 transition-transform group-hover:translate-x-0.5" />
              </button>
              <div class="flex gap-2">
                <button
                  onclick={() => { tab = 'activate'; }}
                  class="flex-1 rounded-xl border border-[var(--rg-border)] py-2 text-xs font-medium text-[var(--rg-fg)] transition-colors hover:border-[var(--rg-accent)]/40 hover:bg-white/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50"
                >
                  国内卡密激活
                </button>
                <button
                  onclick={() => { tab = 'login'; }}
                  class="flex-1 rounded-xl border border-[var(--rg-border)] py-2 text-xs font-medium text-[var(--rg-fg)] transition-colors hover:border-[var(--rg-accent)]/40 hover:bg-white/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/50"
                >
                  本地登录
                </button>
              </div>
            {/if}
          </div>
        {:else if tab === 'login'}
          <form class="space-y-3" onsubmit={(e) => { e.preventDefault(); void doLogin(); }}>
            <label class="block">
              <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">邮箱</span>
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
              <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">密码</span>
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
              登录
            </button>
          </form>
        {:else}
          <form class="space-y-3" onsubmit={(e) => { e.preventDefault(); void doActivate(); }}>
            <p class="text-xs leading-relaxed text-[var(--rg-fg-muted)]">
              在面包多购买卡密后，于此输入激活 Pro。
              <button type="button" onclick={() => openExternal(MBD_URL)} class="text-[var(--rg-accent)] hover:underline">
                前往面包多 ↗
              </button>
            </p>
            {#if !loggedIn}
              <p class="rounded-lg bg-amber-500/10 px-3 py-2 text-xs text-amber-400">
                请先在「本地登录」标签登录账号，再激活卡密。
              </p>
            {/if}
            <label class="block">
              <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">卡密</span>
              <input
                bind:value={licenseKey}
                required
                class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 font-mono text-sm uppercase tracking-wider text-[var(--rg-fg)] outline-none transition-colors focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
                placeholder="RIDGE-XXXX-XXXX-XXXX"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-xs text-[var(--rg-fg-muted)]">用户名（如尚未设置，可一并设定）</span>
              <input
                bind:value={activateUsername}
                class="w-full rounded-lg border border-[var(--rg-border)] bg-black/20 px-3 py-2 text-sm text-[var(--rg-fg)] outline-none transition-colors focus:border-[var(--rg-accent)]/60 focus:ring-2 focus:ring-[var(--rg-accent)]/30"
                placeholder="3-20 位小写字母或数字"
              />
            </label>
            <button
              type="submit"
              disabled={busy || !loggedIn}
              class="flex w-full items-center justify-center gap-2 rounded-xl bg-[var(--rg-accent)] py-2.5 text-sm font-semibold text-white transition-all hover:brightness-110 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/60"
            >
              {#if busy}<Loader2 class="h-4 w-4 animate-spin" />{:else}<KeyRound class="h-4 w-4" />{/if}
              激活
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
