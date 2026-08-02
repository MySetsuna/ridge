import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  createPwaInstallController,
  type PwaInstallOptions,
} from './pwaInstall';

type TestMedia = EventTarget & { matches: boolean };
type TestTarget = EventTarget & { matchMedia: () => TestMedia };

function browserTarget(matches = false): { target: TestTarget; media: TestMedia } {
  const media = Object.assign(new EventTarget(), { matches }) as TestMedia;
  const target = new EventTarget() as TestTarget;
  target.matchMedia = () => media;
  return { target, media };
}

function installEvent(
  outcome: 'accepted' | 'dismissed' = 'accepted',
  prompt: () => Promise<void> | void = () => undefined,
): Event & { prompt: typeof prompt; userChoice: Promise<{ outcome: typeof outcome; platform: string }> } {
  const event = new Event('beforeinstallprompt', { cancelable: true }) as Event & {
    prompt: typeof prompt;
    userChoice: Promise<{ outcome: typeof outcome; platform: string }>;
  };
  event.prompt = prompt;
  event.userChoice = Promise.resolve({ outcome, platform: 'web' });
  return event;
}

afterEach(() => {
  vi.useRealTimers();
});

describe('createPwaInstallController', () => {
  it('captures one event, prevents the default UI, and consumes prompt once', async () => {
    const { target } = browserTarget();
    const prompt = vi.fn(() => Promise.resolve());
    const controller = createPwaInstallController({ target, navigator: {} });
    const event = installEvent('accepted', prompt);

    target.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(controller.getSnapshot()).toMatchObject({ status: 'available', canPrompt: true });

    const first = controller.promptInstall();
    const second = controller.promptInstall();
    expect(second).toBe(first);
    await expect(first).resolves.toEqual({ outcome: 'accepted', platform: 'web' });
    expect(prompt).toHaveBeenCalledTimes(1);
    expect(controller.getSnapshot()).toMatchObject({ status: 'installed', canPrompt: false });

    // The one-shot event is not re-usable after the first prompt.
    await expect(controller.promptInstall()).resolves.toEqual({ outcome: 'unavailable' });
    controller.destroy();
  });

  it('reports dismissal and prompt failures without leaving a pending event', async () => {
    const dismissed = browserTarget();
    const dismissedController = createPwaInstallController({ target: dismissed.target, navigator: {} });
    dismissed.target.dispatchEvent(installEvent('dismissed'));
    await expect(dismissedController.promptInstall()).resolves.toMatchObject({ outcome: 'dismissed' });
    expect(dismissedController.getSnapshot()).toMatchObject({ status: 'dismissed', canPrompt: false });
    dismissedController.destroy();

    const failed = browserTarget();
    const error = new Error('prompt failed');
    const failedController = createPwaInstallController({ target: failed.target, navigator: {} });
    failed.target.dispatchEvent(installEvent('accepted', () => { throw error; }));
    await expect(failedController.promptInstall()).resolves.toMatchObject({ outcome: 'error', error });
    expect(failedController.getSnapshot()).toMatchObject({ status: 'unsupported', canPrompt: false });
    failedController.destroy();
  });

  it('times out an abandoned userChoice promise', async () => {
    vi.useFakeTimers();
    const { target } = browserTarget();
    const event = new Event('beforeinstallprompt', { cancelable: true }) as Event & {
      prompt: () => void;
      userChoice: PromiseLike<{ outcome: 'accepted'; platform: string }>;
    };
    event.prompt = vi.fn();
    event.userChoice = new Promise(() => {});
    const controller = createPwaInstallController({ target, navigator: {}, promptTimeoutMs: 25 });

    target.dispatchEvent(event);
    const result = controller.promptInstall();
    await vi.advanceTimersByTimeAsync(25);
    await expect(result).resolves.toEqual({ outcome: 'timeout' });
    expect(controller.getSnapshot()).toMatchObject({ status: 'timeout', canPrompt: false });
    controller.destroy();
  });

  it('handles standalone, iOS manual install, and display-mode changes', () => {
    const ios = browserTarget();
    const iosController = createPwaInstallController({
      target: ios.target,
      navigator: { userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)' },
    });
    expect(iosController.getSnapshot()).toMatchObject({ status: 'ios-manual', isIos: true });
    ios.target.dispatchEvent(installEvent());
    expect(iosController.getSnapshot()).toMatchObject({ status: 'ios-manual', canPrompt: false });
    iosController.destroy();

    const appInstalled = browserTarget();
    const appInstalledController = createPwaInstallController({ target: appInstalled.target, navigator: {} });
    appInstalled.target.dispatchEvent(installEvent());
    expect(appInstalledController.getSnapshot().status).toBe('available');
    appInstalled.target.dispatchEvent(new Event('appinstalled'));
    expect(appInstalledController.getSnapshot()).toMatchObject({ status: 'installed', canPrompt: false });
    appInstalledController.destroy();

    const installed = browserTarget(true);
    const installedController = createPwaInstallController({ target: installed.target, navigator: {} });
    expect(installedController.getSnapshot()).toMatchObject({ status: 'installed', isStandalone: true });
    installed.media.matches = false;
    installed.media.dispatchEvent(new Event('change'));
    expect(installedController.getSnapshot()).toMatchObject({ status: 'unsupported', isStandalone: false });
    installedController.destroy();
  });

  it('removes listeners once and ignores events after destroy', () => {
    const { target } = browserTarget();
    const remove = vi.spyOn(target, 'removeEventListener');
    const controller = createPwaInstallController({ target, navigator: {} });
    const snapshots: string[] = [];
    controller.subscribe((snapshot) => snapshots.push(snapshot.status));
    controller.destroy();
    controller.destroy();
    target.dispatchEvent(installEvent());

    expect(remove).toHaveBeenCalledTimes(2);
    expect(controller.getSnapshot().status).toBe('unsupported');
    expect(snapshots).toEqual(['unsupported']);
  });

  it('does not claim a malformed event is installable', () => {
    const { target } = browserTarget();
    const controller = createPwaInstallController({ target, navigator: {} } satisfies PwaInstallOptions);
    target.dispatchEvent(new Event('beforeinstallprompt', { cancelable: true }));
    expect(controller.getSnapshot()).toMatchObject({ status: 'unsupported', canPrompt: false });
    controller.destroy();
  });
});
