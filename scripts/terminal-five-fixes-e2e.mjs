import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { createRequire } from 'node:module';
import { chromium } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { resolveCdpPort, DEV_USER_DATA_DIR } from './cdp-port.mjs';

const testRequire = createRequire(import.meta.resolve('@playwright/test'));
const core = path.dirname(testRequire.resolve('playwright-core/package.json'));
const { PNG } = testRequire(path.join(core, 'lib/utilsBundle.js'));
const viewerRoot = path.resolve('artifacts/terminal-five-fixes/viewer');
const viewerEntry = 'virtual:ridge-viewer-acceptance';
await build({
  configFile: false, logLevel: 'error',
  plugins: [svelte({ compilerOptions: { dev: false } }), {
    name: 'ridge-viewer-acceptance',
    resolveId: (id) => id.endsWith(viewerEntry) ? '\0' + viewerEntry : null,
    load: (id) => id === '\0' + viewerEntry ? `
      import { mount } from 'svelte';
      import FileViewer from ${JSON.stringify(path.resolve('src/remote/lib/FileViewer.svelte').replaceAll('\\', '/'))};
      export function mountFixture(target, provider) {
        return mount(FileViewer, { target, props: { provider, kind: 'file',
          path: '/fixture/styles.ts', onClose() {} } });
      }` : null,
  }],
  resolve: { alias: { '$lib': path.resolve('src/lib') }, dedupe: ['svelte'] },
  define: { 'import.meta.env.RIDGE_WEB_REMOTE': 'false' },
  build: { outDir: viewerRoot, lib: { entry: viewerEntry, formats: ['es'],
    fileName: 'viewer', cssFileName: 'viewer' } },
});

const root = path.resolve('build');
const chunks = path.join(root, '_app/immutable/chunks');
const storeChunk = fs.readdirSync(chunks).find((name) => name.endsWith('.js') &&
  fs.readFileSync(path.join(chunks, name), 'utf8').includes('read_file_for_editor') &&
  fs.readFileSync(path.join(chunks, name), 'utf8').includes('openDiffTab'));
assert.ok(storeChunk, 'production file editor store chunk must exist');
const types = { '.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html',
  '.wasm': 'application/wasm', '.woff2': 'font/woff2', '.svg': 'image/svg+xml', '.png': 'image/png' };
const server = http.createServer((request, response) => {
  const pathname = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
  const fixture = pathname === '/_terminal_fixture/font'
    ? 'C:/Windows/Fonts/consola.ttf'
    : /^\/_viewer_fixture\/viewer\.(js|css)$/.test(pathname)
      ? path.join(viewerRoot, path.basename(pathname))
    : /^\/_terminal_fixture\/ridge_term(?:_render|_font)?(?:_bg\.wasm|\.js)$/.test(pathname)
      ? path.resolve(pathname.includes('ridge_term_render') ? 'packages/ridge-term-render/pkg' : pathname.includes('ridge_term_font') ? 'packages/ridge-term-font/pkg' : 'packages/ridge-term/pkg', path.basename(pathname)) : null;
  const file = fixture ?? path.resolve(root, pathname === '/' ? 'index.html' : `.${pathname}`);
  if ((!fixture && !file.startsWith(root + path.sep)) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
    response.writeHead(404).end(); return;
  }
  response.writeHead(200, { 'Content-Type': types[path.extname(file)] ?? 'application/octet-stream',
    'Cache-Control': 'no-store' });
  fs.createReadStream(file).pipe(response);
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const browser = await chromium.launch({ headless: true,
  args: [...(process.argv.includes('--webgpu') ? ['--enable-unsafe-webgpu', '--disable-gpu-sandbox'] : ['--enable-unsafe-swiftshader', '--use-angle=swiftshader']),
    `--explicitly-allowed-ports=${server.address().port}`] });
let nativeBrowser = null;
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  await page.addInitScript(() => {
    window.__styleViolations = [];
    document.addEventListener('securitypolicyviolation', (event) => {
      if (event.violatedDirective.startsWith('style-src')) {
        window.__styleViolations.push(event.violatedDirective);
      }
    });
  });
  const origin = `http://127.0.0.1:${server.address().port}`;
  await page.goto(`${origin}/?e2e=1`);
  await page.evaluate(async (url) => {
    const module = await import(url);
    const store = Object.values(module).find((value) => value &&
      typeof value.openFile === 'function' && typeof value.updateContent === 'function');
    if (!store) throw new Error('file editor store export unavailable');
    await store.openFile('/e2e/styles.ts');
    store.updateContent('/e2e/styles.ts', 'const sample = 42;\nconsole.log(sample);');
  }, `${origin}/_app/immutable/chunks/${storeChunk}`);
  await page.locator('.monaco-editor .view-lines').first().waitFor({ timeout: 30_000 });
  await page.waitForFunction(() => [...document.querySelectorAll('.view-line')]
    .some((line) => line.textContent.includes('sample')));
  const baseline = process.argv.includes('--baseline');
  if (!baseline) await page.waitForFunction(() => {
    const tokens = [...document.querySelectorAll('.view-line span')];
    const keyword = tokens.find((span) => span.textContent === 'const');
    const number = tokens.find((span) => span.textContent === '42');
    return keyword && number && getComputedStyle(keyword).color !== getComputedStyle(number).color;
  });
  // No native backend in this fixture: expose the already-mounted editor.
  await page.evaluate(() => document.getElementById('brand-loader')?.remove());
  const appearance = await page.evaluate(() => {
    const tokens = [...document.querySelectorAll('.view-line span')]
      .filter((span) => span.textContent.trim()).map((span) => ({
        text: span.textContent, color: getComputedStyle(span).color,
      }));
    return { tokens, colors: [...new Set(tokens.map((token) => token.color))],
      violations: window.__styleViolations, adoptedSheets: document.adoptedStyleSheets.length };
  });
  fs.mkdirSync('artifacts/terminal-five-fixes', { recursive: true });
  const artifact = `artifacts/terminal-five-fixes/editor-production-${baseline ? 'baseline' : 'fixed'}`;
  await page.screenshot({ path: `${artifact}.png` });
  fs.writeFileSync(`${artifact}.json`, JSON.stringify(appearance, null, 2));
  console.log(JSON.stringify(appearance));
  if (!baseline) assert.ok(appearance.colors.length > 1, 'syntax tokens retain distinct theme colors under production CSP');
  if (!baseline) {
    const viewer = await browser.newPage({ viewport: { width: 375, height: 740 } });
    await viewer.goto(origin);
    await viewer.evaluate(async (base) => {
      document.body.replaceChildren();
      const css = document.createElement('link');
      css.rel = 'stylesheet'; css.href = `${base}/_viewer_fixture/viewer.css`;
      document.head.appendChild(css);
      await new Promise((resolve, reject) => { css.onload = resolve; css.onerror = reject; });
      for (const [name, value] of Object.entries({ '--rg-bg': '#181818', '--rg-fg': '#eeeeee',
        '--rg-fg-muted': '#aaaaaa', '--rg-surface': '#222222', '--rg-accent': '#80aaff' })) {
        document.documentElement.style.setProperty(name, value);
      }
      const { mountFixture } = await import(`${base}/_viewer_fixture/viewer.js`);
      const provider = { readFile: async () => 'const first = 1;\r\n  second();\r\n',
        writeFile: async () => { throw new Error('read fixture must never write'); } };
      mountFixture(document.body, provider);
    }, origin);
    await viewer.locator('.code-row').nth(1).waitFor();
    assert.deepEqual(await viewer.locator('.ln').allTextContents(), ['1', '2']);
    const readStyle = await viewer.locator('.lc').first().evaluate((element) => ({
      whiteSpace: getComputedStyle(element).whiteSpace,
      fontFamily: getComputedStyle(element).fontFamily,
      rowWidth: element.parentElement.getBoundingClientRect().width,
    }));
    assert.equal(readStyle.whiteSpace, 'pre-wrap');
    assert.match(readStyle.fontFamily, /monospace/);
    assert.ok(readStyle.rowWidth <= 375);
    await viewer.screenshot({ path: 'artifacts/terminal-five-fixes/remote-viewer-production-fixed.png' });
    await viewer.locator('.v-header button').first().click();
    await viewer.locator('textarea.editor').waitFor();
    assert.match(await viewer.locator('textarea.editor').evaluate((element) => getComputedStyle(element).fontFamily), /monospace/);
    console.log(JSON.stringify({ remoteViewerStyles: 'PASS', ...readStyle }));
    let gpu;
    if (process.argv.includes('--native-webgpu')) {
      const config = JSON.parse(fs.readFileSync(path.join(DEV_USER_DATA_DIR, 'tauri-dev-cdp.config.json'), 'utf8'));
      const version = await fetch(`http://127.0.0.1:${resolveCdpPort()}/json/version`).then(response => response.json());
      nativeBrowser = await chromium.connectOverCDP(version.webSocketDebuggerUrl);
      gpu = nativeBrowser.contexts().flatMap(context => context.pages()).find(page => page.url().startsWith(config.build.devUrl));
      assert.ok(gpu, 'isolated current-source native page required');
      await gpu.goto(config.build.devUrl + '?gpu-acceptance=' + Date.now(), { waitUntil: 'domcontentloaded' });
      await gpu.waitForFunction(() => !!window.__TAURI_INTERNALS__, undefined, { timeout: 20000 });
      await gpu.waitForFunction(async () => { const url = performance.getEntriesByType('resource').map(entry => entry.name).find(url => url.includes('/terminal/manager.ts')); if (!url) return false; const {TerminalManager} = await import(url); return !!TerminalManager.tryInstance()?.wasmReady; }, undefined, { timeout: 30000 });
    } else {
      gpu = await browser.newPage({ viewport: { width: 800, height: 400 } });
      await gpu.goto(origin);
    }
    const metrics = await gpu.evaluate(async ({ base, native }) => {
      let wasm, renderModule, fonts;
      if (native) {
        const runtime = await import('/packages/remote/src/shared/terminal/wasmRuntime.ts');
        [wasm, renderModule, fonts] = await Promise.all([import('/packages/ridge-term/pkg/ridge_term.js'), runtime.loadRendererRuntime(), runtime.loadSelectedFontRuntime()]);
        await wasm.default({ module_or_path: '/packages/ridge-term/pkg/ridge_term_bg.wasm' });
        const service = await import('/packages/remote/src/shared/terminal/fontDataService.ts');
        await service.loadTerminalFonts('Consolas', fonts.installFontData);
      } else {
        [wasm, renderModule, fonts] = await Promise.all([import(`${base}/_terminal_fixture/ridge_term.js`), import(`${base}/_terminal_fixture/ridge_term_render.js`), import(`${base}/_terminal_fixture/ridge_term_font.js`)]);
        await Promise.all([wasm.default(), renderModule.default(), fonts.default()]);
        fonts.installFontData(new Uint8Array(await (await fetch(`${base}/_terminal_fixture/font`)).arrayBuffer()));
      }
      renderModule.installFontRasterizer((width, height) => new fonts.FontRasterizer(width, height));
      if (!native) { document.body.replaceChildren(); document.body.style.margin = '0'; }
      const canvas = document.createElement('canvas');
      canvas.dataset.terminalFixture = 'true';
      if (native) Object.assign(canvas.style, { position: 'fixed', top: '0px', left: '0px', zIndex: '2147483647' });
      document.body.appendChild(canvas);
      const host = await renderModule.SurfaceHostHandle.init(canvas);
      const raw = await renderModule.RenderHandle.newWithWebgpuFirst(canvas, host.clone());
      let revision = 0;
      const renderer = new Proxy(raw, { get(target, key) {
        if (key === "render") return kernel => { const packet = kernel.presentationFrame(revision); if (packet) revision = target.applyPresentation(packet); return target.render(); };
        const value = target[key]; return typeof value === "function" ? value.bind(target) : value;
      } });
      const [width, height] = renderer.configure('Consolas', 15, 1);
      canvas.style.width = `${width * 16}px`;
      canvas.style.height = `${height * 6}px`;
      host.resize(width * 16, height * 6, 1);
      renderer.resize(width * 16, height * 6, 1);
      const kernel = new wasm.TerminalKernel(6, 16, 100);
      kernel.feed(new TextEncoder().encode('\x1b[?25l'));
      const row = (index) => `\x1b[48;2;${40 + index * 5};120;170m${' '.repeat(16)}`;
      kernel.feed(new TextEncoder().encode(Array.from({ length: 6 }, (_, i) => row(i)).join('\r\n')));
      const render = async () => {
        host.beginFrame(new Uint8Array([0, 0, 0, 255]));
        renderer.render(kernel);
        host.endFrame();
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      };
      window.__terminalFixture = { kernel, renderer, host, row, render };
      await render();
      return { width, height, backend: renderer.backendName() };
    }, { base: origin, native: process.argv.includes('--native-webgpu') });
    if (process.argv.includes('--webgpu') || process.argv.includes('--native-webgpu')) assert.equal(metrics.backend, 'WebGPU', 'WebGPU acceptance must use WebGPU');
    const canvas = gpu.locator('canvas[data-terminal-fixture]');
    for (let step = 0; step <= 12; step++) {
      if (step) await gpu.evaluate(async (index) => {
        const fixture = window.__terminalFixture;
        fixture.kernel.feed(new TextEncoder().encode(`\r\n${fixture.row(index)}`));
        await fixture.render();
      }, step + 5);
      const image = PNG.sync.read(await canvas.screenshot({ scale: 'css' }));
      const reds = Array.from({ length: 6 }, (_, row) => {
        const offset = (Math.floor((row + 0.5) * metrics.height) * image.width + Math.floor(image.width / 2)) * 4;
        return image.data[offset];
      });
      fs.writeFileSync(`artifacts/terminal-five-fixes/gpu-row-order-${step}.png`, PNG.sync.write(image));
      assert.ok(reds.every((red, row) => row === 0 || red > reds[row - 1]),
        `${metrics.backend} painted row order reversed at step ${step}: ${reds}`);
    }
    console.log(JSON.stringify({ gpuRowOrder: 'PASS', ...metrics }));
    await gpu.evaluate(async () => {
      const fixture = window.__terminalFixture;
      fixture.renderer.applyTheme({ cursor: '#ff00ff' });
      fixture.kernel.feed(new TextEncoder().encode('\x1b[3;3H\x1b[?25h'));
      await fixture.render();
      fixture.renderer.setPresentationCursorSuppressed(true);
      fixture.kernel.feed(new TextEncoder().encode('\x1b[6;1H\r\n'));
      await fixture.render();
    });
    const cursorProbe = async (expectedRow) => {
      const row = await gpu.evaluate(() => window.__terminalFixture.renderer.presentedCursorRow());
      assert.equal(row, expectedRow, 'cursor probe follows its content row');
      const image = PNG.sync.read(await canvas.screenshot({ scale: 'css' }));
      for (let sample = 0; sample < 6; sample++) {
        const offset = (Math.floor((sample + 0.5) * metrics.height) * image.width +
          Math.floor(2.5 * metrics.width)) * 4;
        const magenta = image.data[offset] === 255 && image.data[offset + 1] === 0 &&
          image.data[offset + 2] === 255;
        assert.equal(magenta, sample === expectedRow, 'painted cursor matches the probe');
      }
    };
    await cursorProbe(1);
    await gpu.evaluate(async () => {
      window.__terminalFixture.kernel.scrollUp(1);
      await window.__terminalFixture.render();
    });
    await cursorProbe(2);
    await gpu.evaluate(async () => {
      window.__terminalFixture.kernel.scrollUp(10);
      await window.__terminalFixture.render();
    });
    await cursorProbe(-1);
    await gpu.evaluate(async () => {
      window.__terminalFixture.kernel.scrollToBottom();
      await window.__terminalFixture.render();
    });
    await cursorProbe(1);
    console.log(JSON.stringify({ gpuCursorContentAnchor: 'PASS', backend: metrics.backend }));
    await gpu.evaluate(async () => {
      const f = window.__terminalFixture;
      f.renderer.setPresentationCursorSuppressed(false);
      f.renderer.setFocused(true);
      f.kernel.feed(new TextEncoder().encode('\x1b[0m\x1b[2J\x1b[H\x1b[?12l\x1b[?25h'));
      await f.render();
    });
    for (let frame = 0; frame < 60; frame++) {
      const expected = { row: frame % 6, col: Math.floor(frame / 6) % 16 };
      const probe = await gpu.evaluate(async (position) => {
        const f = window.__terminalFixture;
        f.kernel.feed(new TextEncoder().encode(`\x1b[${position.row + 1};${position.col + 1}H`));
        await f.render();
        return { row: f.renderer.presentedCursorRow(), col: f.renderer.presentedCursorCol() };
      }, expected);
      assert.deepEqual(probe, expected, `cursor movement frame ${frame} is current`);
      const image = PNG.sync.read(await canvas.screenshot({ scale: 'css' }));
      const offset = (Math.floor((expected.row + 0.5) * metrics.height) * image.width + Math.floor((expected.col + 0.5) * metrics.width)) * 4;
      assert.deepEqual([...image.data.subarray(offset, offset + 3)], [255, 0, 255], `cursor movement frame ${frame} painted`);
    }
    console.log(JSON.stringify({ gpuCursorMovementFrames: 60, paintedFrames: 60, backend: metrics.backend }));
    await gpu.evaluate(async () => {
      const f = window.__terminalFixture;
      f.kernel.feed(new TextEncoder().encode('\x1b[0m\x1b[2J\x1b[H\x1b[?25l\x1b]8;;https://example.com\x07LINK\x1b]8;;\x07\x1b[3;1H\x1b[4mTEXT\x1b[0m'));
      f.renderer.setLinkUnderlines([]);
      await f.render();
    });
    const plainLinks = PNG.sync.read(await canvas.screenshot({ scale: 'css' }));
    await gpu.evaluate(async () => {
      const f = window.__terminalFixture;
      f.kernel.feed(new TextEncoder().encode('\x1b[1;1H    \x1b[1;1HLINK\x1b[3;1H\x1b[0mTEXT'));
      await f.render();
    });
    const noUnderline = PNG.sync.read(await canvas.screenshot({ scale: 'css' }));
    let sgrPixels = 0;
    for (let y = 0; y < plainLinks.height; y++) for (let x = 0; x < plainLinks.width; x++) {
      const offset = (y * plainLinks.width + x) * 4;
      if (!plainLinks.data.subarray(offset, offset + 4).equals(noUnderline.data.subarray(offset, offset + 4))) {
        sgrPixels++;
        assert.ok(y >= 2 * metrics.height && y < 3 * metrics.height && x < 4 * metrics.width, 'OSC8 adds no painted underline; only removing SGR underline changes pixels');
      }
    }
    assert.ok(sgrPixels > 0, 'SGR underline actually paints');
    await gpu.evaluate(async () => {
      const f = window.__terminalFixture;
      f.kernel.feed(new TextEncoder().encode('\x1b[3;1H\x1b[4mTEXT\x1b[0m'));
      await f.render();
    });

    await gpu.evaluate(async () => {
      const f = window.__terminalFixture;
      f.renderer.setLinkUnderlines([{ row: 0, c0: 0, c1: 4 }]);
      await f.render();
    });
    const hoveredLink = PNG.sync.read(await canvas.screenshot({ scale: 'css' }));
    let changed = 0;
    for (let y = 0; y < hoveredLink.height; y++) for (let x = 0; x < hoveredLink.width; x++) {
      const offset = (y * hoveredLink.width + x) * 4;
      if (!hoveredLink.data.subarray(offset, offset + 4).equals(plainLinks.data.subarray(offset, offset + 4))) {
        changed++;
        assert.ok(y < metrics.height && x < 4 * metrics.width, 'only the hovered link receives underline; SGR underline stays unchanged');
      }
    }
    assert.ok(changed > 0, 'hover underline actually paints');
    await gpu.evaluate(async () => { const f = window.__terminalFixture; f.renderer.setLinkUnderlines([]); await f.render(); });
    const clearedLink = PNG.sync.read(await canvas.screenshot({ scale: 'css' }));
    assert.ok(clearedLink.data.equals(plainLinks.data), 'leaving a link restores every pixel');
    console.log(JSON.stringify({ gpuLinkHoverRegions: 'PASS', osc8NoAutomaticUnderline: true, sgrUnderlinePreserved: true, backend: metrics.backend }));
    if (nativeBrowser) await gpu.evaluate(() => { const f = window.__terminalFixture; f.renderer.free(); f.host.free(); f.kernel.free(); document.querySelector('canvas[data-terminal-fixture]')?.remove(); delete window.__terminalFixture; });

  }
} finally {
  if (nativeBrowser) { for (const context of nativeBrowser.contexts()) for (const page of context.pages()) await page.evaluate(() => document.querySelector('canvas[data-terminal-fixture]')?.remove()).catch(() => {}); await nativeBrowser.close(); }
  await browser.close();
  await new Promise((resolve) => server.close(resolve));
}
