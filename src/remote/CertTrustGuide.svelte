<script lang="ts">
  // Collapsible "trust this device's certificate" helper shown on the
  // verification screen. The Ridge remote server presents a leaf cert signed by
  // a local root CA (src-tauri/src/remote/tls.rs); installing + trusting that CA
  // once per device silences the browser's "not secure" warning and unlocks the
  // secure-context features (WebGPU terminal rendering). The CA is downloadable
  // from the server at /ridge-ca.crt (DER, for mobile install) and /ridge-ca.pem
  // (for desktop trust stores).

  type Platform = 'ios' | 'android' | 'desktop';

  function detectPlatform(): Platform {
    const ua = navigator.userAgent || '';
    const isIOS =
      /iPad|iPhone|iPod/.test(ua) ||
      // iPadOS 13+ reports as MacIntel but has touch points.
      ((navigator.platform === 'MacIntel' || /Macintosh/.test(ua)) && navigator.maxTouchPoints > 1);
    if (isIOS) return 'ios';
    if (/Android/.test(ua)) return 'android';
    return 'desktop';
  }

  let open = $state(false);
  const platform: Platform = detectPlatform();
  // Over plain HTTP there is no cert to trust — installing one won't help.
  const secure = typeof window !== 'undefined' ? window.isSecureContext : true;

  const steps: Record<Platform, string[]> = {
    ios: [
      '点击下方「下载证书」，在弹窗中选择「允许」下载描述文件。',
      '打开「设置 → 通用 → VPN 与设备管理」，安装「Ridge Remote Local CA」描述文件。',
      '打开「设置 → 通用 → 关于本机 → 证书信任设置」，为「Ridge Remote Local CA」打开完全信任。',
      '返回浏览器刷新本页，地址栏的「不安全」警告即消除。',
    ],
    android: [
      '点击下方「下载证书」保存 ridge-remote-ca.crt。',
      '打开「设置 → 安全 → 加密与凭据 → 安装证书 → CA 证书」（部分机型在「从存储设备安装」）。',
      '选择刚下载的 ridge-remote-ca.crt 完成安装。',
      '返回浏览器刷新本页。',
    ],
    desktop: [
      'Windows：下载 .crt → 双击 → 安装证书 → 选择「受信任的根证书颁发机构」→ 完成。',
      'macOS：下载 .pem → 用「钥匙串访问」导入到「系统」→ 双击证书并设为「始终信任」。',
      'Linux/其他：将 .pem 导入浏览器或系统的受信任根证书。',
      '重启浏览器后刷新本页。',
    ],
  };
</script>

<div class="cert">
  <button class="toggle" onclick={() => (open = !open)} aria-expanded={open}>
    <span class="lock">🔒</span>
    <span>浏览器提示「不安全」？安装证书消除警告</span>
    <span class="chev" class:rot={open}>▾</span>
  </button>

  {#if open}
    <div class="panel">
      <p class="intro">
        本机使用自动生成的本地证书加密远程连接。安装并信任一次后，
        浏览器不再提示「不安全」，并解锁完整功能（含 GPU 加速终端渲染）。
      </p>

      {#if !secure}
        <p class="insecure">
          当前为 HTTP 连接，证书暂不可用。请确认桌面端已启用 HTTPS 后通过
          <code>https://</code> 地址重新打开本页。
        </p>
      {/if}

      <ol class="steps">
        {#each steps[platform] as step}
          <li>{step}</li>
        {/each}
      </ol>

      <div class="actions">
        {#if platform === 'ios'}
          <!-- iOS: a plain navigation triggers the profile-install prompt;
               a `download` attribute can route it to Files instead. -->
          <a class="dl primary" href="/ridge-ca.crt">下载证书</a>
        {:else}
          <a class="dl primary" href="/ridge-ca.crt" download="ridge-remote-ca.crt">下载证书 (.crt)</a>
        {/if}
        <a class="dl ghost" href="/ridge-ca.pem" download="ridge-remote-ca.pem">.pem</a>
      </div>
    </div>
  {/if}
</div>

<style>
  .cert{width:100%;max-width:340px;margin-top:16px}
  .toggle{display:flex;align-items:center;gap:8px;width:100%;background:transparent;border:none;color:var(--rg-fg-muted);font-size:13px;cursor:pointer;padding:8px 4px;text-align:left}
  .toggle:hover{color:var(--rg-fg)}
  .lock{font-size:13px;flex:0 0 auto}
  .toggle span:nth-child(2){flex:1 1 auto}
  .chev{flex:0 0 auto;transition:transform .2s;font-size:11px}
  .chev.rot{transform:rotate(180deg)}
  .panel{margin-top:8px;background:var(--rg-surface);border:1px solid var(--rg-border-bright);border-radius:10px;padding:16px;text-align:left}
  .intro{color:var(--rg-fg-muted);font-size:12.5px;line-height:1.6;margin-bottom:12px}
  .insecure{color:var(--rg-ansi-red);font-size:12px;line-height:1.5;margin-bottom:12px}
  .insecure code{background:rgba(255,255,255,.08);padding:1px 5px;border-radius:4px}
  .steps{margin:0 0 14px;padding-left:18px;color:var(--rg-fg);font-size:12.5px;line-height:1.7}
  .steps li{margin-bottom:6px}
  .actions{display:flex;gap:10px;align-items:center}
  .dl{display:inline-flex;align-items:center;justify-content:center;height:40px;padding:0 18px;border-radius:9px;font-size:14px;font-weight:600;text-decoration:none;transition:opacity .2s,background .2s}
  .dl.primary{flex:1 1 auto;background:var(--rg-accent);color:#fff}
  .dl.primary:hover{opacity:.9}
  .dl.ghost{flex:0 0 auto;background:transparent;border:1px solid var(--rg-border-bright);color:var(--rg-fg-muted);padding:0 14px}
  .dl.ghost:hover{color:var(--rg-fg);border-color:var(--rg-fg-muted)}
</style>
