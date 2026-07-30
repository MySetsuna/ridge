# Mobile Remote `runtime.lastError` audit

- Requirement: `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01`
- Scope: the sustained warning observed on the phone Remote page:
  `Unchecked runtime.lastError: A listener indicated an asynchronous response by returning true, but the message channel closed before a response was received`
- Status: project source excluded; exact browser/injected owner awaits phone evidence.

## Source result

Repository-wide source and call-path inspection found no use of:

- `chrome.runtime.onMessage`
- `chrome.tabs.sendMessage`
- `chrome.runtime.lastError`
- `browser.runtime`
- `sendResponse`
- an extension listener that returns `true`

The owned mobile/PWA path is:

`src/remote/main.ts` → service-worker registration → `src/service-worker.ts` → standard Service Worker `Client.postMessage`.

That API is not Chrome Extension Messaging and has no `sendResponse`/`return true` contract. Therefore the repository cannot currently be shown to originate this warning, and adding response wrappers or Console suppression to business code would be false remediation.

## Current root-cause classification

The warning text is generated when a Chrome Extension Messaging listener promises an asynchronous response, then its Tab/Frame/Port closes before that response arrives. With no matching project API, the likely owner is browser-, OEM-, content-blocker-, password-manager-, DevTools-, or other injected extension code. This remains a hypothesis until the affected phone identifies the first warning's script URL and a controlled A/B reproduces it.

## Phone verification matrix

1. Capture browser name/version, OS version, exact Remote URL, first warning source URL, frame and repetition count.
2. Open the same URL in a clean profile or incognito mode where extensions/injection are disabled.
3. Re-enable content blockers, password managers, userscript tools, remote-debugging helpers and other injectors one at a time.
4. If the browser supports no extensions, compare stock Chrome clean profile with the OEM/extension-capable browser or WebView where the warning appears.
5. Use remote DevTools to inspect the first occurrence; later repeats are aggregation noise, not independent roots.
6. Record before/after counts for the same connect, switch-pane, type, resize, background/foreground and reconnect scenario.

## Acceptance

- Clean profile produces no sustained warning.
- Re-enabling one injector reproduces it and identifies its script/extension owner; or a project-owned URL proves repository ownership and opens a new approved code slice.
- Remote terminal input, resize, reconnect and PWA storage messaging still work.
- No Console filter, warning suppression, meaningless retry or duplicate listener is added.

## Evidence still required

- Affected-phone screenshot or exported Console entry containing the first source URL.
- Clean-profile/incognito A/B counts.
- Browser/OS version and enabled injector list.

Until these exist, the accurate result is “project code excluded; environment attribution pending”, not “warning fixed”.
