<script lang="ts">
  // 连接远端主机对话框。整窗居中（use:portal 逃出侧边栏的 backdrop-filter 包含块），
  // 只分两类**接入通道**——与网页端一致，不再暴露 ridge/rdg 的实现细节：
  //   • 局域网 (LAN)：直连，无需登录，只需「地址 + TOTP 验证码」。
  //   • 公网       ：经云端中继，需先登录账户（复用「公网远控」的云端登录态）；
  //                  已登录后只需「设备名 + TOTP 验证码」。
  // 凭据(TOTP)只用于本次握手，不落库。
  import { Globe, Wifi, X, Loader2 } from 'lucide-svelte';
  import { portal } from '$lib/actions/portal';
  import { connectHost, hostConnectProgress } from '$lib/stores/hosts';
  import { alertDialog } from '../RidgeDialog.svelte';
  import { cloudAuth, isLoggedIn, login as cloudLogin, loginViaBrowser } from '@ridge/remote/shared/cloud/auth';

  interface Props {
    open: boolean;
  }
  let { open = $bindable() }: Props = $props();

  let channel = $state<'lan' | 'public'>('lan');
  let label = $state('');
  let addr = $state('');
  let totp = $state('');

  // 公网登录态（复用云端 auth store，与「公网远控」tab 同一账户）。
  const loggedIn = $derived(isLoggedIn($cloudAuth));
  const account = $derived($cloudAuth.user?.username || $cloudAuth.user?.email || '');
  let loginBusy = $state(false);
  let email = $state('');
  let password = $state('');
  let loginErr = $state('');

  // 是否已可填写「地址 + 验证码」并接入：LAN 恒可；公网须先登录。
  const readyForFields = $derived(channel === 'lan' || loggedIn);

  function reset(): void {
    channel = 'lan';
    label = '';
    addr = '';
    totp = '';
    email = '';
    password = '';
    loginErr = '';
  }

  function close(): void {
    reset();
    open = false;
  }

  // 浏览器授权登录（公网远控 tab 的主登录路径）。
  async function doBrowserLogin(): Promise<void> {
    loginBusy = true;
    loginErr = '';
    try {
      await loginViaBrowser();
    } catch (e) {
      loginErr = e instanceof Error ? e.message : String(e);
    } finally {
      loginBusy = false;
    }
  }

  // 邮箱 + 密码登录（同一云端 auth，公网远控 tab 的备选路径）。
  async function doEmailLogin(): Promise<void> {
    if (!email.trim() || !password) return;
    loginBusy = true;
    loginErr = '';
    try {
      await cloudLogin(email.trim(), password);
      password = '';
    } catch (e) {
      loginErr = e instanceof Error ? e.message : String(e);
    } finally {
      loginBusy = false;
    }
  }

  async function submit(): Promise<void> {
    if (channel === 'public' && !loggedIn) {
      await alertDialog({ title: '需要登录', message: '公网接入需先登录云端账户。' });
      return;
    }
    if (!addr.trim()) {
      await alertDialog({ title: '缺少地址', message: '请填写主机地址（ip:port）。' });
      return;
    }
    if (!totp.trim()) {
      await alertDialog({ title: '缺少验证码', message: '请填写主机的 TOTP 验证码。' });
      return;
    }
    const request = {
      channel,
      label,
      addr,
      totp,
    };
    // Connection discovery can take seconds; move progress to the Hosts panel.
    close();
    try {
      await connectHost('remote', request.label, request.addr, request.totp, request.channel);
    } catch (e) {
      await alertDialog({ title: '接入失败', message: e instanceof Error ? e.message : String(e) });
    }
  }
</script>

{#if open}
  <!-- use:portal 把整个遮罩移到 <body>，以整个客户端窗口为参照居中（逃出侧边栏容器）。 -->
  <div
    use:portal={{ id: 'host-connect' }}
    class="fixed inset-0 z-[200] flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm"
    role="presentation"
    onclick={close}
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="w-[min(440px,92vw)] rounded-2xl border border-[var(--rg-border)] bg-[var(--rg-surface-2)] text-[var(--rg-fg)] shadow-2xl"
      role="dialog"
      aria-label="连接远端主机"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
    >
      <header class="flex h-12 items-center justify-between border-b border-[var(--rg-border)] px-4">
        <span class="text-[13px] font-semibold">连接远端主机</span>
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded-lg text-[var(--rg-fg-muted)] hover:bg-white/[0.06] hover:text-[var(--rg-fg)]"
          onclick={close}
          aria-label="关闭"
        >
          <X class="h-4 w-4" />
        </button>
      </header>

      <div class="space-y-3 px-4 py-3">
        <!-- 接入通道：局域网 / 公网 -->
        <div class="flex gap-2">
          <button
            type="button"
            class="flex h-9 flex-1 items-center justify-center gap-1.5 rounded-lg border text-[12px] transition-colors {channel ===
            'lan'
              ? 'border-[var(--rg-accent)] bg-[var(--rg-accent)]/10 text-[var(--rg-accent)]'
              : 'border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:bg-white/[0.04]'}"
            onclick={() => (channel = 'lan')}
          >
            <Wifi class="h-4 w-4" /> 局域网 (LAN)
          </button>
          <button
            type="button"
            class="flex h-9 flex-1 items-center justify-center gap-1.5 rounded-lg border text-[12px] transition-colors {channel ===
            'public'
              ? 'border-[var(--rg-accent)] bg-[var(--rg-accent)]/10 text-[var(--rg-accent)]'
              : 'border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:bg-white/[0.04]'}"
            onclick={() => (channel = 'public')}
          >
            <Globe class="h-4 w-4" /> 公网
          </button>
        </div>

        {#if channel === 'public' && !loggedIn}
          <!-- 公网登录 gate：复用云端登录态（与公网远控同一账户）。 -->
          <div class="space-y-2 rounded-lg border border-[var(--rg-border)] bg-[var(--rg-surface)] p-3">
            <p class="text-[11px] leading-relaxed text-[var(--rg-fg-muted)]">
              公网整机接入需先登录同一云端账户。登录后填写该账户下设备名与 TOTP 验证码。
            </p>
            <button
              type="button"
              disabled={loginBusy}
              onclick={doBrowserLogin}
              class="flex h-9 w-full items-center justify-center gap-1.5 rounded-lg bg-[var(--rg-accent)] text-[12px] font-medium text-black hover:opacity-90 disabled:opacity-40"
            >
              {#if loginBusy}<Loader2 class="h-4 w-4 animate-spin" />{/if} 浏览器登录
            </button>
            <div class="text-center text-[10px] text-[var(--rg-fg-muted)]">或用邮箱密码登录</div>
            <input
              bind:value={email}
              type="email"
              placeholder="邮箱"
              class="h-9 w-full rounded-lg border border-[var(--rg-border)] bg-[var(--rg-bg)] px-2.5 text-[13px] outline-none focus:border-[var(--rg-accent)]"
            />
            <input
              bind:value={password}
              type="password"
              placeholder="密码"
              onkeydown={(e) => { if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); void doEmailLogin(); } }}
              class="h-9 w-full rounded-lg border border-[var(--rg-border)] bg-[var(--rg-bg)] px-2.5 text-[13px] outline-none focus:border-[var(--rg-accent)]"
            />
            <button
              type="button"
              disabled={loginBusy || !email.trim() || !password}
              onclick={doEmailLogin}
              class="h-8 w-full rounded-lg border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] disabled:opacity-40"
            >
              登录
            </button>
            {#if loginErr}
              <p class="text-[11px] text-[var(--rg-ansi-red)]">{loginErr}</p>
            {/if}
          </div>
        {:else}
          {#if channel === 'public'}
            <p class="text-[11px] text-[var(--rg-fg-muted)]">已登录：<span class="text-[var(--rg-fg)]">{account}</span></p>
          {/if}
          <label class="block">
            <span class="text-[11px] text-[var(--rg-fg-muted)]">{channel === 'public' ? '设备名' : '地址'}</span>
            <input
              bind:value={addr}
              placeholder={channel === 'public' ? 'ridge-desktop' : '192.168.1.5:9528'}
              class="mt-1 h-9 w-full rounded-lg border border-[var(--rg-border)] bg-[var(--rg-surface)] px-2.5 text-[13px] outline-none focus:border-[var(--rg-accent)]"
            />
          </label>

          <label class="block">
            <span class="text-[11px] text-[var(--rg-fg-muted)]">TOTP 验证码（不会被保存）</span>
            <input
              bind:value={totp}
              inputmode="numeric"
              autocomplete="off"
              placeholder="6 位验证码"
              class="mt-1 h-9 w-full rounded-lg border border-[var(--rg-border)] bg-[var(--rg-surface)] px-2.5 text-[13px] outline-none focus:border-[var(--rg-accent)]"
            />
          </label>

          <label class="block">
            <span class="text-[11px] text-[var(--rg-fg-muted)]">别名（可选）</span>
            <input
              bind:value={label}
              placeholder="工位台机"
              class="mt-1 h-9 w-full rounded-lg border border-[var(--rg-border)] bg-[var(--rg-surface)] px-2.5 text-[13px] outline-none focus:border-[var(--rg-accent)]"
            />
          </label>

          <p class="text-[10px] leading-relaxed text-[var(--rg-fg-muted)]">
            {channel === 'lan'
              ? '局域网直连，无需登录——填对地址与 TOTP 验证码即可接入。'
              : '公网经 Cloud E2EE 中继；relay 与 host 均校验同账号，异账号整机接入会被拒绝。'}
          </p>
        {/if}
      </div>

      <footer class="flex h-14 items-center justify-end gap-2 border-t border-[var(--rg-border)] px-4">
        <button
          type="button"
          class="h-8 rounded-lg px-3 text-[12px] text-[var(--rg-fg-muted)] hover:bg-white/[0.06]"
          onclick={close}
        >
          取消
        </button>
        <button
          type="button"
          disabled={($hostConnectProgress?.phase !== 'error' && $hostConnectProgress !== null) || !readyForFields || !addr.trim() || !totp.trim()}
          class="h-8 rounded-lg bg-[var(--rg-accent)] px-3 text-[12px] font-medium text-black hover:opacity-90 disabled:opacity-40"
          onclick={submit}
        >
          接入
        </button>
      </footer>
    </div>
  </div>
{/if}
