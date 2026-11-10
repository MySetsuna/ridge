import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import { chromium } from '@playwright/test';
import { build } from 'vite';
import { trustedMonacoStyles } from './monaco-trusted-styles.mjs';

function fixture(monaco, styles, EditorWorker, ContextView) {
  window.MonacoEnvironment = { getWorker: () => new EditorWorker() };
  const unrelated = new CSSStyleSheet();
  unrelated.replaceSync('body { margin: 0; }');
  document.adoptedStyleSheets = [...document.adoptedStyleSheets, unrelated];
  const models = [
    monaco.editor.createModel('hello\nworld', 'plaintext'),
    monaco.editor.createModel('before', 'plaintext'),
    monaco.editor.createModel('after', 'plaintext'),
  ];
  const editor = monaco.editor.create(document.getElementById('editor'), { model: models[0], minimap: { enabled: false } });
  const diff = monaco.editor.createDiffEditor(document.getElementById('diff'), { renderSideBySide: true });
  diff.setModel({ original: models[1], modified: models[2] });
  window.__editor = editor;
  window.__diff = diff;
  window.__setTheme = (background) => {
    monaco.editor.defineTheme('ridge-csp-test', { base: 'vs-dark', inherit: true, rules: [], colors: { 'editor.background': background } });
    monaco.editor.setTheme('ridge-csp-test');
  };
  window.__setTheme('#102030');
  window.__styleLifecycle = async () => {
    const before = document.adoptedStyleSheets.length;
    const host = document.createElement('div');
    document.body.append(host);
    const shadow = host.attachShadow({ mode: 'open' });
    const probe = document.createElement('div');
    probe.className = 'style-probe';
    shadow.append(probe);
    const own = styles.createTrustedMonacoStyle(shadow);
    own.textContent = '.style-probe { color: rgb(11, 13, 17); }';
    styles.attachTrustedMonacoStyle(own, shadow);
    const shadowColor = getComputedStyle(probe).color;
    own.sheet.insertRule('.style-probe { background: rgb(19, 23, 29); }', own.sheet.cssRules.length);
    const dynamicColor = getComputedStyle(probe).backgroundColor;
    own.sheet.deleteRule(1);
    own.remove();
    const shadowReleased = shadow.adoptedStyleSheets.length === 0;

    const frame = document.createElement('iframe');
    const loaded = new Promise(resolve => frame.addEventListener('load', resolve, { once: true }));
    frame.src = 'about:blank';
    document.body.append(frame);
    await loaded;
    const target = frame.contentDocument;
    const frameProbe = target.createElement('div');
    frameProbe.className = 'clone-probe';
    target.body.append(frameProbe);
    const source = styles.createTrustedMonacoStyle(document.head);
    source.textContent = '.clone-probe { color: rgb(31, 37, 41); }';
    styles.attachTrustedMonacoStyle(source, document.head);
    const clone = styles.cloneTrustedMonacoStyle(source, target);
    styles.attachTrustedMonacoStyle(clone, target.head);
    const cloneColor = frame.contentWindow.getComputedStyle(frameProbe).color;
    const changed = new Promise(resolve => {
      const observer = new MutationObserver(() => { clone.textContent = source.textContent; observer.disconnect(); resolve(); });
      observer.observe(source, { childList: true, characterData: true, subtree: true });
    });
    source.textContent = '.clone-probe { color: rgb(43, 47, 53); }';
    await changed;
    const cloneUpdated = frame.contentWindow.getComputedStyle(frameProbe).color;
    clone.remove();
    source.remove();
    const cloneReleased = target.adoptedStyleSheets.length === 0;
    frame.remove();
    host.remove();
    return { shadowColor, dynamicColor, shadowReleased, cloneColor, cloneUpdated, cloneReleased, unrelatedPreserved: document.adoptedStyleSheets.includes(unrelated), unchangedCount: document.adoptedStyleSheets.length === before };
  };
  window.__shadowEditorLifecycle = () => {
    const host = document.createElement('div');
    document.body.append(host);
    const shadow = host.attachShadow({ mode: 'open' });
    const container = document.createElement('div');
    container.style.cssText = 'width:400px;height:180px';
    shadow.append(container);
    const model = monaco.editor.createModel('shadow editor', 'plaintext');
    const shadowEditor = monaco.editor.create(container, { model, useShadowDOM: true });
    const mounted = shadow.adoptedStyleSheets.length;
    shadowEditor.dispose();
    model.dispose();
    const released = shadow.adoptedStyleSheets.length;
    host.remove();
    return { mounted, released };
  };
  window.__contextViewLifecycle = () => {
    const context = new ContextView(document.body, 3);
    const root = context.getViewElement().getRootNode();
    const adopted = root.adoptedStyleSheets.length;
    context.dispose();
    return { adopted, detached: !root.host.isConnected };
  };
  editor.focus();
  window.__monacoReady = true;
}

const unpatched = process.argv.includes('--unpatched');
const root = path.resolve('.');
const runtime = path.resolve('src/lib/monaco/trustedThemeStyles.ts').replaceAll('\\', '/');
const entry = [
  "import * as monaco from 'monaco-editor';",
  "import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';",
  "import { ContextView } from 'monaco-editor/esm/vs/base/browser/ui/contextview/contextview.js';",
  'import * as styles from ' + JSON.stringify(runtime) + ';',
  '(' + fixture.toString() + ')(monaco, styles, EditorWorker, ContextView);',
].join('\n');
const result = await build({
  configFile: false, root, logLevel: 'error',
  plugins: [
    ...unpatched ? [] : [trustedMonacoStyles()],
    { name: 'ridge-monaco-csp-fixture', resolveId: id => id === 'ridge-monaco-csp-fixture' ? '\0ridge-monaco-csp-fixture' : null, load: id => id === '\0ridge-monaco-csp-fixture' ? entry : null },
  ],
  build: { write: false, minify: false, cssCodeSplit: false, rollupOptions: { input: 'ridge-monaco-csp-fixture', output: { entryFileNames: 'entry.js' } } },
});
const output = Array.isArray(result) ? result.flatMap(item => item.output) : result.output;
const assets = new Map(output.map(item => ['/' + item.fileName, { body: item.type === 'chunk' ? item.code : item.source, type: item.fileName.endsWith('.js') ? 'text/javascript' : item.fileName.endsWith('.css') ? 'text/css' : 'application/octet-stream' }]));
const css = output.filter(item => item.fileName.endsWith('.css')).map(item => '<link rel="stylesheet" href="/' + item.fileName + '">').join('');
const policy = "default-src 'none'; script-src 'self'; style-src 'self'; style-src-attr 'unsafe-inline'; worker-src 'self' blob:; font-src 'self' data:; img-src 'self' data:; connect-src 'self'; frame-src 'self'";
const server = http.createServer((req, res) => {
  const pathname = new URL(req.url, 'http://localhost').pathname;
  if (pathname === '/') {
    res.writeHead(200, { 'content-type': 'text/html', 'content-security-policy': policy });
    res.end('<!doctype html><html><head>' + css + '</head><body><div id="editor" style="width:1100px;height:300px"></div><div id="diff" style="width:1100px;height:300px"></div><script type="module" src="/entry.js"></script></body></html>');
  } else if (assets.has(pathname)) {
    const asset = assets.get(pathname);
    res.writeHead(200, { 'content-type': asset.type });
    res.end(asset.body);
  } else { res.writeHead(pathname === '/favicon.ico' ? 204 : 404); res.end(); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
let browser;
try {
  browser = await chromium.launch({ channel: 'chrome', headless: true });
  const page = await browser.newPage({ viewport: { width: 1200, height: 750 } });
  await page.addInitScript(() => {
    window.__cspViolations = [];
    document.addEventListener('securitypolicyviolation', event => window.__cspViolations.push(event.effectiveDirective));
  });
  await page.goto('http://127.0.0.1:' + server.address().port, { waitUntil: 'networkidle' });
  await page.waitForFunction(() => window.__monacoReady === true);
  await fs.mkdir('artifacts', { recursive: true });
  await page.screenshot({ path: 'artifacts/monaco-csp-' + (unpatched ? 'before' : 'after') + '.png' });
  assert.deepEqual(await page.evaluate(() => window.__cspViolations), [], 'Monaco emitted CSP violations');
  const color = () => page.locator('#editor .monaco-editor-background').first().evaluate(element => getComputedStyle(element).backgroundColor);
  assert.equal(await color(), 'rgb(16, 32, 48)');
  await page.keyboard.type('X');
  assert.equal(await page.evaluate(() => window.__editor.getValue()), 'Xhello\nworld');
  await page.waitForFunction(() => window.__diff.getLineChanges()?.length > 0);
  await page.evaluate(() => window.__setTheme('#405060'));
  assert.equal(await color(), 'rgb(64, 80, 96)');
  const lifecycle = await page.evaluate(() => window.__styleLifecycle());
  assert.deepEqual(lifecycle, { shadowColor: 'rgb(11, 13, 17)', dynamicColor: 'rgb(19, 23, 29)', shadowReleased: true, cloneColor: 'rgb(31, 37, 41)', cloneUpdated: 'rgb(43, 47, 53)', cloneReleased: true, unrelatedPreserved: true, unchangedCount: true });
  const shadow = await page.evaluate(() => window.__shadowEditorLifecycle());
  assert.ok(shadow.mounted > 0);
  assert.equal(shadow.released, 0);
  const contextView = await page.evaluate(() => window.__contextViewLifecycle());
  assert.ok(contextView.adopted > 0);
  assert.equal(contextView.detached, true);
  assert.deepEqual(await page.evaluate(() => window.__cspViolations), [], 'Editor interactions emitted CSP violations');
  await page.evaluate(() => {
    const style = document.createElement('style');
    style.textContent = 'body { color: red; }';
    document.head.append(style);
    const script = document.createElement('script');
    script.textContent = 'window.__untrustedScriptRan = true';
    document.head.append(script);
  });
  await page.waitForFunction(() => window.__cspViolations.length >= 2);
  const denied = await page.evaluate(() => ({ violations: window.__cspViolations, scriptRan: !!window.__untrustedScriptRan }));
  assert.equal(denied.scriptRan, false);
  assert.ok(denied.violations.includes('style-src-elem'));
  assert.ok(denied.violations.includes('script-src-elem'));
  console.log(JSON.stringify({ passed: true, realMonacoEditing: true, diff: true, theme: true, lifecycle, shadow, contextView, unauthorizedStylesAndScriptsRejected: true }));
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
}
