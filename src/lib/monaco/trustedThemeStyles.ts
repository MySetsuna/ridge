type StyleRoot = Document | ShadowRoot;
interface TrustedStyle { sheet: CSSStyleSheet; root: StyleRoot | null }
const styles = new WeakMap<HTMLStyleElement, TrustedStyle>();

/** Keep Monaco's element/CSSOM API and its mutation observers, but never mount
 * its inline style element. Only the trusted library's stylesheet is adopted. */
function prepare(style: HTMLStyleElement): HTMLStyleElement {
  const view = style.ownerDocument.defaultView;
  if (!view || typeof view.CSSStyleSheet?.prototype.replaceSync !== 'function') {
    throw new Error('Monaco requires constructed stylesheets under this CSP');
  }
  const text = Object.getOwnPropertyDescriptor(view.Node.prototype, 'textContent')!;
  const sheet = new view.CSSStyleSheet({ media: style.media || 'screen' });
  sheet.replaceSync(style.textContent ?? '');
  const state: TrustedStyle = { sheet, root: null };
  const remove = style.remove.bind(style);
  styles.set(style, state);
  Object.defineProperties(style, {
    sheet: { get: () => sheet },
    textContent: {
      get: () => text.get!.call(style),
      set: (value: string | null) => {
        const css = value ?? '';
        text.set!.call(style, css);
        sheet.replaceSync(css);
      },
    },
    remove: { value: () => {
      if (state.root) {
        state.root.adoptedStyleSheets = state.root.adoptedStyleSheets.filter(item => item !== sheet);
        state.root = null;
      }
      remove();
    } },
  });
  return style;
}

export function createTrustedMonacoStyle(container: Node): HTMLStyleElement {
  const doc = container.nodeType === 9 ? container as Document : container.ownerDocument!;
  return prepare(doc.createElement('style'));
}

export function attachTrustedMonacoStyle(style: HTMLStyleElement, container: Node): void {
  const state = styles.get(style);
  const candidate = container.getRootNode();
  const root = ('adoptedStyleSheets' in candidate ? candidate : container.ownerDocument) as StyleRoot;
  if (!state || !root || !('adoptedStyleSheets' in root)) throw new Error('Monaco style root unavailable');
  if (state.root && state.root !== root) {
    state.root.adoptedStyleSheets = state.root.adoptedStyleSheets.filter(item => item !== state.sheet);
  }
  if (!root.adoptedStyleSheets.includes(state.sheet)) root.adoptedStyleSheets = [...root.adoptedStyleSheets, state.sheet];
  state.root = root;
}

export function cloneTrustedMonacoStyle(style: HTMLStyleElement, doc: Document): HTMLStyleElement {
  const clone = doc.createElement('style');
  for (const attribute of style.attributes) clone.setAttribute(attribute.name, attribute.value);
  clone.textContent = style.textContent;
  return prepare(clone);
}
