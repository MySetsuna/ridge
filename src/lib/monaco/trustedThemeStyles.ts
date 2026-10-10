let observer: MutationObserver | null = null;

/** Monaco generates this theme in JS; production CSP rejects its inline tag.
 * Adopt only that trusted theme through CSSOM, preserving the CSP policy. */
export function ensureTrustedMonacoStyles(): void {
  if (observer || typeof document === 'undefined' || !document.head ||
    typeof MutationObserver === 'undefined' || typeof CSSStyleSheet === 'undefined' ||
    !('adoptedStyleSheets' in document)) return;

  const themes = new Map<HTMLStyleElement, { sheet: CSSStyleSheet; text: string }>();
  const synchronize = () => {
    const styles = new Set(document.head.querySelectorAll<HTMLStyleElement>('style.monaco-colors'));
    for (const [style, { sheet }] of themes) {
      if (!styles.has(style) || style.sheet) {
        document.adoptedStyleSheets = document.adoptedStyleSheets.filter((item) => item !== sheet);
        themes.delete(style);
      }
    }
    for (const style of styles) {
      if (style.sheet) continue;
      const text = style.textContent ?? '';
      let theme = themes.get(style);
      if (!theme) {
        theme = { sheet: new CSSStyleSheet(), text: '' };
        document.adoptedStyleSheets = [...document.adoptedStyleSheets, theme.sheet];
        themes.set(style, theme);
      }
      if (theme.text !== text) {
        theme.sheet.replaceSync(text);
        theme.text = text;
      }
    }
  };
  observer = new MutationObserver(synchronize);
  observer.observe(document.head, { childList: true, subtree: true, characterData: true });
  synchronize();
}
