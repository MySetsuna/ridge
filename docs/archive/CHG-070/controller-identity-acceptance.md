# Controller identity migration receipt

Approved through CHG-070 and `BUILD-9d6648e8b1e7`. Native Cloud now initializes Runtime's controller identity before connection. Only authenticated Native callers can import a checked 32-byte legacy seed; no signing RPC is added. Host identity and Browser Remote signing ownership remain unchanged.

- Actual browser IndexedDB/localStorage: eight isolated cases passed, including absence without database creation, either source, matching sources, malformed sources, conflicts and denied reads. Zero storage writes; originals remain intact. Receipt: `C:\Users\12867\AppData\Local\Temp\ridge-controller-browser-fowB6l\result.json`.
- Runtime import/restart: two integration cases passed, covering premature connection rejection, Remote denial, malformed input, same-key concurrent import, different-key conflict, canonical corruption, preservation of previous Runtime Cloud identity and byte-identical Host identity. Log: `.webview2-dev-cdp/070-controller-identity-runtime.log`.
- Encrypted persistence: 17 DeviceIdentity tests passed, including complete no-overwrite publication, same-key replay, conflicting concurrent imports and interrupted temporary files. Log: `070-controller-identity-core.log`.
- Legacy grant: the real Rust trust-proof handler accepts a signature made by the persisted/reloaded legacy controller identity for its original public-key grant. Log: `070-controller-identity-trust.log`.
- Actual Cloud transport: the existing owned relay/browser/Rust RTC acceptance now imports a legacy seed before Native Cloud connection. E2EE, TOTP, stored-grant reconnect without another TOTP, canonical terminal delivery, original-child reply, foreign projection, scoped denial and corrupt-pin preservation passed. Log: `070-controller-cloud-owner.log`.

No original user browser storage or installed identity was read or migrated. Browser-source, Native migration and Host trust receipts cover their respective real boundaries; they do not claim an upgrade of an installed user's WebView profile.
