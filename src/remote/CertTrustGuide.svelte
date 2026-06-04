<script lang="ts">
  // Collapsible "trust this device's certificate" helper shown on the
  // verification screen. The Ridge remote server presents a leaf cert signed by
  // a local root CA (src-tauri/src/remote/tls.rs); installing + trusting that CA
  // once per device silences the browser's "not secure" warning and unlocks the
  // secure-context features (WebGPU terminal rendering). The CA is downloadable
  // from the server at /ridge-ca.crt (DER, for mobile install) and /ridge-ca.pem
  // (for desktop trust stores).

  import { t, tr } from '$lib/i18n';

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

  const stepKeys: Record<Platform, string[]> = {
    ios: [
      'mobile.certIosStep1',
      'mobile.certIosStep2',
      'mobile.certIosStep3',
      'mobile.certIosStep4',
    ],
    android: [
      'mobile.certAndroidStep1',
      'mobile.certAndroidStep2',
      'mobile.certAndroidStep3',
      'mobile.certAndroidStep4',
    ],
    desktop: [
      'mobile.certDesktopStep1',
      'mobile.certDesktopStep2',
      'mobile.certDesktopStep3',
      'mobile.certDesktopStep4',
    ],
  };
</script>

<div class="cert">
  <button class="toggle" onclick={() => (open = !open)} aria-expanded={open}>
    <span class="lock">🔒</span>
    <span>{$t('mobile.certToggleLabel')}</span>
    <span class="chev" class:rot={open}>▾</span>
  </button>

  {#if open}
    <div class="panel">
      <p class="intro">{$t('mobile.certIntro')}</p>

      {#if !secure}
        {@const parts = $t('mobile.certInsecure').split('{code}')}
        <p class="insecure">
          {parts[0]}<code>https://</code>{parts[1] ?? ''}
        </p>
      {/if}

      <ol class="steps">
        {#each stepKeys[platform] as key}
          <li>{$t(key)}</li>
        {/each}
      </ol>

      <div class="actions">
        {#if platform === 'ios'}
          <!-- iOS: a plain navigation triggers the profile-install prompt;
               a `download` attribute can route it to Files instead. -->
          <a class="dl primary" href="/ridge-ca.crt">{$t('mobile.downloadCert')}</a>
        {:else}
          <a class="dl primary" href="/ridge-ca.crt" download="ridge-remote-ca.crt">{$t('mobile.downloadCertCrt')}</a>
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
