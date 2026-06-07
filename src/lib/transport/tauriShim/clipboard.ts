// src/lib/transport/tauriShim/clipboard.ts
//
// Browser stand-in for `@tauri-apps/plugin-clipboard-manager`. Backed by the
// Web Clipboard API (available because the remote server is served over HTTPS,
// a secure context). readText may reject without a user gesture / permission;
// callers already handle clipboard failures.

export async function writeText(text: string): Promise<void> {
  await navigator.clipboard.writeText(text);
}

export async function readText(): Promise<string> {
  return navigator.clipboard.readText();
}
