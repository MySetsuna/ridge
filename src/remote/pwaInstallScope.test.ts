import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const mainSource = readFileSync(new URL('./MainApp.svelte', import.meta.url), 'utf8');
const appSource = readFileSync(new URL('./App.svelte', import.meta.url), 'utf8');
const updateSource = readFileSync(new URL('./lib/PwaUpdateBanner.svelte', import.meta.url), 'utf8');
const indexSource = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
const authSource = readFileSync(new URL('./AuthScreen.svelte', import.meta.url), 'utf8');
const cloudAuthSource = readFileSync(new URL('./CloudAuthScreen.svelte', import.meta.url), 'utf8');
const bottomBarSource = readFileSync(new URL('./BottomTabBar.svelte', import.meta.url), 'utf8');
const sidebarSource = readFileSync(new URL('./lib/RemoteSidebar.svelte', import.meta.url), 'utf8');
const bootstrapSource = readFileSync(new URL('./main.ts', import.meta.url), 'utf8');
const configSource = readFileSync(new URL('../../vite.remote.config.js', import.meta.url), 'utf8');

describe('Remote PWA installation scope', () => {
  it('shares update actions across the preconnection and connected screens without loading MainApp early', () => {
    expect(appSource).toContain('{#if !mainAppVisible}');
    for (const source of [appSource, mainSource]) {
      expect(source).toContain('<PwaUpdateBanner bind:height={pwaBannerHeight} />');
      expect(source).not.toContain("applyPwaUpdate('discard')");
    }
    expect(appSource).toContain("retryableLazyImport(() => import('./MainApp.svelte'))");
    expect(mainSource).toContain('style:--rg-pwa-update-height=');
    expect(updateSource).toContain("applyPwaUpdate('save')");
    expect(updateSource).toContain("applyPwaUpdate('discard')");
    expect(updateSource).toContain('onclick={deferPwaUpdate}');
    expect(updateSource).toContain('disabled={$pwaUpdateBusy || $pwaUpdateSaving}');
    expect(updateSource).toContain('min-height:44px');
    for (const source of [authSource, cloudAuthSource, appSource]) {
      expect(source).toContain('inset:var(--rg-preconnect-update-height,0px) 0 0');
    }
  });

  it('leaves installation to browser-native UI without an in-app prompt', () => {
    for (const token of ['PwaInstallAction', 'beforeinstallprompt', 'appinstalled', 'deferredPrompt']) {
      expect(mainSource).not.toContain(token);
      expect(bootstrapSource).not.toContain(token);
    }
    expect(existsSync(new URL('./PwaInstallAction.svelte', import.meta.url))).toBe(false);
    expect(existsSync(new URL('./lib/pwaInstall.ts', import.meta.url))).toBe(false);
  });

  it('keeps the manifest/service worker foundations for browser installation', () => {
    expect(bootstrapSource).toContain("registerSW({");
    expect(configSource).toContain('VitePWA({');
    expect(configSource).toContain("display: 'standalone'");
    expect(configSource).toContain("scope: '/'");
    expect(configSource).toContain("globPatterns: ['**/*']");
    expect(indexSource).toContain('viewport-fit=cover');
    // The Vite mobile build has no `/_app/immutable/entry/startup.js`; a stale
    // preload would waste a request (and a round trip on weak mobile links).
    expect(indexSource).not.toContain('/_app/immutable/entry/startup.js');
  });

  it('keeps notch and home-indicator controls inside browser and standalone PWA safe areas', () => {
    expect(mainSource).toContain('env(safe-area-inset-top');
    expect(mainSource).toContain('.conn-banner-safe{');
    expect(mainSource).toContain('.conn-banner{');
    expect(mainSource).toContain('max(12px,env(safe-area-inset-right,0px))');
    expect(mainSource).toContain('constant(safe-area-inset-top)');
    expect(mainSource).toContain('flex-wrap:wrap');
    expect(mainSource).toContain('@media (display-mode:standalone) and (orientation:portrait)');
    expect(mainSource).toContain('max(64px,env(safe-area-inset-top,0px))');
    expect(mainSource).toContain('margin-top:env(safe-area-inset-top,0px)');
    expect(mainSource).toContain('data-ridge-pwa="standalone"');
    expect(mainSource).toContain('height:100dvh');
    expect(bottomBarSource).toContain('env(safe-area-inset-bottom');
    expect(bottomBarSource).toContain('margin-top:auto');
    expect(bottomBarSource).toContain('box-sizing:border-box');
    expect(bottomBarSource).toContain('data-ridge-pwa="standalone"');
    expect(sidebarSource).toContain('env(safe-area-inset-top');
    expect(sidebarSource).toContain('env(safe-area-inset-bottom');
    expect(sidebarSource).toContain('data-ridge-pwa="standalone"');
  });

  it('detects iOS standalone mode before the first Svelte paint', () => {
    expect(bootstrapSource).toContain("navigator as Navigator & { standalone?: boolean }");
    expect(bootstrapSource).toContain("document.documentElement.dataset.ridgePwa = 'standalone'");
  });

  it('keeps reconnect and failure diagnostics visible on auth fallback screens', () => {
    for (const source of [authSource, cloudAuthSource]) {
      expect(source).toContain('env(safe-area-inset-top,0px)');
      expect(source).toContain('env(safe-area-inset-bottom,0px)');
      expect(source).toContain('constant(safe-area-inset-top)');
      expect(source).toContain('@media (display-mode:standalone) and (orientation:portrait)');
      expect(source).toContain('max(64px,calc(24px + env(safe-area-inset-top,0px)))');
      expect(source).toContain('data-ridge-pwa="standalone"');
      expect(source).toContain('overflow-y:auto');
      expect(source).toContain('box-sizing:border-box');
      expect(source).toContain('justify-content:safe center');
    }
  });
});
