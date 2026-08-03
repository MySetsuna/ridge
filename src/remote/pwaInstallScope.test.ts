import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const mainSource = readFileSync(new URL('./MainApp.svelte', import.meta.url), 'utf8');
const indexSource = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
const authSource = readFileSync(new URL('./AuthScreen.svelte', import.meta.url), 'utf8');
const cloudAuthSource = readFileSync(new URL('./CloudAuthScreen.svelte', import.meta.url), 'utf8');
const bottomBarSource = readFileSync(new URL('./BottomTabBar.svelte', import.meta.url), 'utf8');
const sidebarSource = readFileSync(new URL('./lib/RemoteSidebar.svelte', import.meta.url), 'utf8');
const bootstrapSource = readFileSync(new URL('./main.ts', import.meta.url), 'utf8');
const configSource = readFileSync(new URL('../../vite.remote.config.js', import.meta.url), 'utf8');

describe('Remote PWA installation scope', () => {
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
  });

  it('keeps notch and home-indicator controls inside browser and standalone PWA safe areas', () => {
    expect(mainSource).toContain('env(safe-area-inset-top');
    expect(mainSource).toContain('.conn-banner{');
    expect(mainSource).toContain('min-height:calc(30px + env(safe-area-inset-top,0px))');
    expect(mainSource).toContain('max(12px,env(safe-area-inset-right,0px))');
    expect(mainSource).toContain('constant(safe-area-inset-top)');
    expect(mainSource).toContain('flex-wrap:wrap');
    expect(mainSource).toContain('@media (display-mode:standalone) and (orientation:portrait)');
    expect(mainSource).toContain('max(44px,calc(6px + env(safe-area-inset-top,0px)))');
    expect(mainSource).toContain('height:100dvh');
    expect(bottomBarSource).toContain('env(safe-area-inset-bottom');
    expect(bottomBarSource).toContain('margin-top:auto');
    expect(bottomBarSource).toContain('box-sizing:border-box');
    expect(sidebarSource).toContain('env(safe-area-inset-top');
    expect(sidebarSource).toContain('env(safe-area-inset-bottom');
  });

  it('keeps reconnect and failure diagnostics visible on auth fallback screens', () => {
    for (const source of [authSource, cloudAuthSource]) {
      expect(source).toContain('env(safe-area-inset-top,0px)');
      expect(source).toContain('env(safe-area-inset-bottom,0px)');
      expect(source).toContain('constant(safe-area-inset-top)');
      expect(source).toContain('@media (display-mode:standalone) and (orientation:portrait)');
      expect(source).toContain('max(44px,calc(24px + env(safe-area-inset-top,0px)))');
      expect(source).toContain('overflow-y:auto');
      expect(source).toContain('box-sizing:border-box');
      expect(source).toContain('justify-content:safe center');
    }
  });
});
