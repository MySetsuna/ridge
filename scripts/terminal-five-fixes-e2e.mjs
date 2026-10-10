import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { createRequire } from 'node:module';
import { chromium } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

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
    : /^\/_terminal_fixture\/ridge_term(?:_bg\.wasm|\.js)$/.test(pathname)
      ? path.resolve('packages/ridge-term/pkg', path.basename(pathname)) : null;
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
  args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader',
    `--explicitly-allowed-ports=${server.address().port}`] });
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
    const gpu = await browser.newPage({ viewport: { width: 800, height: 400 } });
    await gpu.goto(origin);
    const metrics = await gpu.evaluate(async (base) => {
      const wasm = await import(`${base}/_terminal_fixture/ridge_term.js`);
      await wasm.default();
      wasm.installFontData(new Uint8Array(await (await fetch(`${base}/_terminal_fixture/font`)).arrayBuffer()));
      document.body.replaceChildren();
      document.body.style.margin = '0';
      const canvas = document.createElement('canvas');
      document.body.appendChild(canvas);
      const host = await wasm.SurfaceHostHandle.init(canvas);
      const renderer = await wasm.RenderHandle.newWithWebgpuFirst(canvas, host.clone());
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
    }, origin);
    const canvas = gpu.locator('canvas');
    for (let step = 0; step <= 12; step++) {
      if (step) await gpu.evaluate(async (index) => {
        const fixture = window.__terminalFixture;
        fixture.kernel.feed(new TextEncoder().encode(`\r\n${fixture.row(index)}`));
        await fixture.render();
      }, step + 5);
      const image = PNG.sync.read(await canvas.screenshot());
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
      const image = PNG.sync.read(await canvas.screenshot());
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
  }
} finally {
  await browser.close();
  await new Promise((resolve) => server.close(resolve));
}
