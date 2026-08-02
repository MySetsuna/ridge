/**
 * Browser/PWA install lifecycle for Remote.
 *
 * `beforeinstallprompt` is deliberately kept out of the component tree: the
 * event is one-shot and may fire before the settings/drawer UI mounts.  This
 * controller captures it once, exposes a small observable snapshot, and makes
 * prompt consumption idempotent.  It has no DOM rendering dependency, so the
 * same contract can be tested in the node Vitest environment.
 */

export type PwaInstallStatus =
  | 'available'
  | 'prompting'
  | 'installed'
  | 'dismissed'
  | 'timeout'
  | 'ios-manual'
  | 'unsupported';

export type PwaInstallOutcome =
  | 'accepted'
  | 'dismissed'
  | 'timeout'
  | 'unavailable'
  | 'error';

export interface PwaInstallSnapshot {
  status: PwaInstallStatus;
  canPrompt: boolean;
  isStandalone: boolean;
  isIos: boolean;
}

export interface PwaInstallResult {
  outcome: PwaInstallOutcome;
  platform?: string;
  error?: unknown;
}

interface BeforeInstallPromptEventLike extends Event {
  prompt(): Promise<void> | void;
  userChoice: PromiseLike<{ outcome: 'accepted' | 'dismissed'; platform?: string }>;
}

interface MediaQueryListLike {
  matches: boolean;
  addEventListener?: (type: 'change', listener: (event: Event) => void) => void;
  removeEventListener?: (type: 'change', listener: (event: Event) => void) => void;
  addListener?: (listener: (event: Event) => void) => void;
  removeListener?: (listener: (event: Event) => void) => void;
}

type InstallEventTarget = EventTarget & {
  matchMedia?: (query: string) => MediaQueryListLike;
};

type PwaNavigator = {
  userAgent?: string;
  platform?: string;
  maxTouchPoints?: number;
  /** iOS Safari's non-standard installed-mode flag. */
  standalone?: boolean;
};

export interface PwaInstallOptions {
  /** Inject a browser-like target for tests; defaults to `window` when present. */
  target?: InstallEventTarget | null;
  /** Inject navigator for tests; defaults to the global navigator when present. */
  navigator?: PwaNavigator | null;
  /** Bound an abandoned `userChoice` promise so UI never stays pending forever. */
  promptTimeoutMs?: number;
}

export interface PwaInstallController {
  getSnapshot(): PwaInstallSnapshot;
  subscribe(listener: (snapshot: PwaInstallSnapshot) => void): () => void;
  /** Must be called from a user gesture. Repeated calls share one promise. */
  promptInstall(): Promise<PwaInstallResult>;
  /** Idempotently unregister all listeners and release the pending event. */
  destroy(): void;
}

const DISPLAY_MODE_QUERY = '(display-mode: standalone)';
const DEFAULT_PROMPT_TIMEOUT_MS = 10_000;
const TIMEOUT = Symbol('pwa-install-timeout');

function isIosNavigator(nav: PwaNavigator | null | undefined): boolean {
  const ua = nav?.userAgent ?? '';
  const platform = nav?.platform ?? '';
  // iPadOS 13+ reports itself as Macintosh while retaining touch points.
  return /iPad|iPhone|iPod/i.test(ua) || (platform === 'MacIntel' && (nav?.maxTouchPoints ?? 0) > 1);
}

function defaultTarget(): InstallEventTarget | null {
  return typeof window !== 'undefined' ? (window as unknown as InstallEventTarget) : null;
}

function defaultNavigator(): PwaNavigator | null {
  return typeof navigator !== 'undefined' ? (navigator as PwaNavigator) : null;
}

function isPromptEvent(value: unknown): value is BeforeInstallPromptEventLike {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<BeforeInstallPromptEventLike>;
  return typeof candidate.prompt === 'function'
    && !!candidate.userChoice
    && typeof (candidate.userChoice as PromiseLike<unknown>).then === 'function';
}

function withTimeout<T>(promise: PromiseLike<T>, timeoutMs: number): Promise<T | typeof TIMEOUT> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const timeout = new Promise<typeof TIMEOUT>((resolve) => {
    timer = setTimeout(() => resolve(TIMEOUT), timeoutMs);
  });
  // Promise.race attaches a rejection handler to the userChoice promise even
  // when the timeout wins, preventing a late browser rejection from becoming
  // an unhandled error.
  return Promise.race([
    Promise.resolve(promise).finally(() => {
      if (timer) clearTimeout(timer);
    }),
    timeout,
  ]);
}

export function createPwaInstallController(options: PwaInstallOptions = {}): PwaInstallController {
  const target = options.target === undefined ? defaultTarget() : options.target;
  const nav = options.navigator === undefined ? defaultNavigator() : options.navigator;
  const isIos = isIosNavigator(nav);
  const timeoutMs = Number.isFinite(options.promptTimeoutMs)
    ? Math.max(1, options.promptTimeoutMs as number)
    : DEFAULT_PROMPT_TIMEOUT_MS;
  const listeners = new Set<(snapshot: PwaInstallSnapshot) => void>();
  const media = target?.matchMedia?.(DISPLAY_MODE_QUERY) ?? null;

  let destroyed = false;
  let pendingPrompt: BeforeInstallPromptEventLike | null = null;
  let promptInFlight: Promise<PwaInstallResult> | null = null;
  let snapshot: PwaInstallSnapshot = {
    status: 'unsupported',
    canPrompt: false,
    isStandalone: false,
    isIos,
  };

  const readStandalone = (): boolean => Boolean(nav?.standalone) || Boolean(media?.matches);

  const publish = (status: PwaInstallStatus): void => {
    if (destroyed) return;
    const next: PwaInstallSnapshot = {
      status,
      canPrompt: status === 'available' && pendingPrompt !== null && !destroyed,
      isStandalone: readStandalone(),
      isIos,
    };
    if (
      next.status === snapshot.status
      && next.canPrompt === snapshot.canPrompt
      && next.isStandalone === snapshot.isStandalone
      && next.isIos === snapshot.isIos
    ) return;
    snapshot = next;
    for (const listener of [...listeners]) {
      try { listener({ ...snapshot }); } catch { /* subscriber failure cannot break lifecycle */ }
    }
  };

  const initialStandalone = readStandalone();
  snapshot = {
    status: initialStandalone ? 'installed' : isIos ? 'ios-manual' : 'unsupported',
    canPrompt: false,
    isStandalone: initialStandalone,
    isIos,
  };

  const onBeforeInstallPrompt = (event: Event): void => {
    const promptEvent = event as BeforeInstallPromptEventLike;
    // Suppress the browser's unsolicited mini-infobar; the UI consumes this
    // exact event from a user gesture via promptInstall().
    try { promptEvent.preventDefault?.(); } catch { /* browser teardown */ }
    if (destroyed || readStandalone() || isIos || pendingPrompt || promptInFlight || snapshot.status === 'installed') return;
    if (!isPromptEvent(promptEvent)) {
      publish('unsupported');
      return;
    }
    pendingPrompt = promptEvent;
    publish('available');
  };

  const onAppInstalled = (): void => {
    if (destroyed) return;
    pendingPrompt = null;
    publish('installed');
  };

  const onDisplayModeChange = (): void => {
    if (destroyed) return;
    if (readStandalone()) {
      pendingPrompt = null;
      publish('installed');
    } else if (!promptInFlight) {
      publish(isIos ? 'ios-manual' : pendingPrompt ? 'available' : 'unsupported');
    }
  };

  if (target) {
    target.addEventListener('beforeinstallprompt', onBeforeInstallPrompt as EventListener);
    target.addEventListener('appinstalled', onAppInstalled as EventListener);
  }
  if (media?.addEventListener) media.addEventListener('change', onDisplayModeChange);
  else if (media?.addListener) media.addListener(onDisplayModeChange);

  const promptInstall = (): Promise<PwaInstallResult> => {
    if (promptInFlight) return promptInFlight;
    if (destroyed || !pendingPrompt || readStandalone() || isIos) {
      return Promise.resolve({ outcome: 'unavailable' });
    }

    const promptEvent = pendingPrompt;
    // Consume exactly once before invoking browser UI. A double click cannot
    // issue a second prompt, even if the browser resolves userChoice slowly.
    pendingPrompt = null;
    publish('prompting');

    const run = (async (): Promise<PwaInstallResult> => {
      try {
        const choicePromise = Promise.resolve(promptEvent.userChoice);
        // Attach a rejection sink before waiting for prompt(). A browser may
        // close its install surface while prompt() is still unresolved.
        void choicePromise.catch(() => undefined);
        const shown = await withTimeout(Promise.resolve(promptEvent.prompt()), timeoutMs);
        if (shown === TIMEOUT) {
          publish('timeout');
          return { outcome: 'timeout' };
        }
        const choice = await withTimeout(choicePromise, timeoutMs);
        if (choice === TIMEOUT) {
          publish('timeout');
          return { outcome: 'timeout' };
        }
        if (choice.outcome === 'accepted') {
          publish('installed');
          return { outcome: 'accepted', platform: choice.platform };
        }
        publish('dismissed');
        return { outcome: 'dismissed', platform: choice.platform };
      } catch (error) {
        publish('unsupported');
        return { outcome: 'error', error };
      } finally {
        promptInFlight = null;
      }
    })();
    promptInFlight = run;
    return run;
  };

  return {
    getSnapshot: () => ({ ...snapshot }),
    subscribe(listener) {
      if (destroyed) return () => {};
      listeners.add(listener);
      try { listener({ ...snapshot }); } catch { /* subscriber failure cannot break lifecycle */ }
      return () => { listeners.delete(listener); };
    },
    promptInstall,
    destroy() {
      if (destroyed) return;
      destroyed = true;
      pendingPrompt = null;
      if (target) {
        target.removeEventListener('beforeinstallprompt', onBeforeInstallPrompt as EventListener);
        target.removeEventListener('appinstalled', onAppInstalled as EventListener);
      }
      if (media?.removeEventListener) media.removeEventListener('change', onDisplayModeChange);
      else if (media?.removeListener) media.removeListener(onDisplayModeChange);
      listeners.clear();
    },
  };
}

// The remote shell is a single browser app, so keep one listener for the
// one-shot browser event.  Components subscribe/unsubscribe without owning
// the event lifecycle; this also covers a prompt emitted before MainApp mounts.
let browserController: PwaInstallController | null = null;

export function getPwaInstallController(): PwaInstallController {
  return browserController ??= createPwaInstallController();
}
