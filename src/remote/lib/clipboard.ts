// §clipboard: robust "write to the CONTROL DEVICE's (phone/browser) system
// clipboard" helper, shared by the terminal copy pill and the file viewer.
//
// Why a shared helper instead of `navigator.clipboard.writeText` inline:
//   1. The async Clipboard API only exists in a SECURE CONTEXT. A LAN link
//      served over plain ws:// (non-TLS) has `navigator.clipboard === undefined`,
//      so the write silently throws and nothing lands on the phone clipboard.
//   2. iOS Safari's `execCommand('copy')` fallback ignores a *readonly*
//      textarea's `select()` (copies nothing). The reliable iOS path is a
//      NON-readonly, contentEditable textarea selected via a Range + explicit
//      setSelectionRange — that is what this helper does.
//
// Must be called from within a user gesture (onclick/ontouchend) — both callers
// are button handlers, which satisfies the Clipboard API's transient-activation
// requirement.

/** Write `text` to the control device's system clipboard. Returns true on a
 *  best-effort success. Never throws. */
export async function writeClipboard(text: string): Promise<boolean> {
  if (!text) return false;
  // Primary: async Clipboard API (secure context + user gesture).
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    /* fall through to the legacy path (permissions / insecure context) */
  }
  return legacyCopy(text);
}

/** Legacy `execCommand('copy')` path, hardened for iOS Safari. */
function legacyCopy(text: string): boolean {
  try {
    const ta = document.createElement('textarea');
    ta.value = text;
    // Off-screen but real (opacity:0 + 1px), non-interactive.
    ta.style.position = 'fixed';
    ta.style.top = '0';
    ta.style.left = '0';
    ta.style.width = '1px';
    ta.style.height = '1px';
    ta.style.padding = '0';
    ta.style.border = '0';
    ta.style.opacity = '0';
    ta.style.pointerEvents = 'none';
    // iOS Safari refuses to copy from a readonly field; make it editable.
    ta.contentEditable = 'true';
    ta.readOnly = false;
    document.body.appendChild(ta);
    try {
      ta.focus();
      ta.select();
      // iOS: bare select() selects nothing → use a Range + setSelectionRange.
      const range = document.createRange();
      range.selectNodeContents(ta);
      const sel = window.getSelection();
      if (sel) {
        sel.removeAllRanges();
        sel.addRange(range);
      }
      try {
        ta.setSelectionRange(0, text.length);
      } catch {
        /* setSelectionRange unsupported on some engines — Range selection stands */
      }
      return document.execCommand('copy');
    } finally {
      document.body.removeChild(ta);
    }
  } catch {
    return false; // clipboard truly unavailable — nothing more we can do
  }
}
