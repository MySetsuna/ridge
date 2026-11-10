import path from 'node:path';
import { fileURLToPath } from 'node:url';

const runtime = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../src/lib/monaco/trustedThemeStyles.ts').replaceAll('\\', '/');
const imports = `import { createTrustedMonacoStyle as ridgeCreateStyle, attachTrustedMonacoStyle as ridgeAttachStyle, cloneTrustedMonacoStyle as ridgeCloneStyle } from ${JSON.stringify(runtime)};\n`;

/** Version-guarded hooks into the installed Monaco library, shared by native
 * and Remote builds. An upstream change fails the build instead of CSP.
 * @param {string} source
 * @param {string} id */
export function transformMonacoStyles(source, id) {
  const file = id.split('?')[0].replaceAll('\\', '/');
  /** @param {string} before @param {string} after */
  const replace = (before, after) => {
    if (source.split(before).length !== 2) throw new Error(`Monaco CSP hook changed: ${file}: ${before}`);
    source = source.replace(before, after);
  };
  if (file.endsWith('/monaco-editor/esm/vs/base/browser/domStylesheets.js')) {
    replace("const style = document.createElement('style');", 'const style = ridgeCreateStyle(container);');
    replace('container.appendChild(style);', 'ridgeAttachStyle(style, container);');
    replace('const clone = globalStylesheet.cloneNode(true);', 'const clone = ridgeCloneStyle(globalStylesheet, targetWindow.document);');
    replace('targetWindow.document.head.appendChild(clone);', 'ridgeAttachStyle(clone, targetWindow.document.head);');
    source = imports + source;
  } else if (file.endsWith('/monaco-editor/esm/vs/base/browser/ui/contextview/contextview.js')) {
    replace("const style = document.createElement('style');", 'const style = ridgeCreateStyle(this.shadowRoot);');
    replace('this.shadowRoot.appendChild(style);', 'ridgeAttachStyle(style, this.shadowRoot);');
    source = imports + source;
  } else if (file.endsWith('/monaco-editor/esm/vs/editor/standalone/browser/standaloneThemeService.js')) {
    replace('this._styleElements.splice(i, 1);', 'styleElement.remove();\n                        this._styleElements.splice(i, 1);');
  } else return null;
  return { code: source, map: null };
}

/** @returns {import('vite').Plugin} */
export function trustedMonacoStyles() {
  return { name: 'ridge-trusted-monaco-styles', apply: 'build', enforce: 'pre', transform: transformMonacoStyles };
}
