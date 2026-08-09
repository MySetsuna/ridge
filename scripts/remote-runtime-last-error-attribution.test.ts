import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';

const probe = await import('./remote-runtime-last-error-attribution.mjs');

const source = readFileSync(resolve(import.meta.dirname, 'remote-runtime-last-error-attribution.mjs'), 'utf8');

describe('Remote runtime.lastError attribution probe', () => {
  it('keeps a clean incognito baseline and disables component extensions', () => {
    expect(source).toContain("'--disable-extensions'");
    expect(source).toContain("'--disable-component-extensions-with-background-pages'");
    expect(source).toContain('Chromium incognito clean profile');
  });

  it('loads exactly one unpacked extension per persistent profile', () => {
    expect(source).toContain('launchPersistentContext');
    expect(source).toContain('`--disable-extensions-except=${extensionPath}`');
    expect(source).toContain('`--load-extension=${extensionPath}`');
    expect(source).toContain('for (const extensionPath of extensions)');
    expect(source).toContain('Deliberately sequential');
  });

  it('fails closed when clean profile warns or extension loading is unverified', () => {
    expect(source).toContain("status: 'warning-in-clean-profile'");
    expect(source).toContain("status: 'extension-load-unverified'");
    expect(source).toContain("'clean-profile-only'");
    expect(source).toContain('attributionComplete: false');
  });

  it('captures Console entries without suppressing the warning', () => {
    expect(source).toContain('page.on(\'console\'');
    expect(source).toContain('runtimeLastErrors');
    expect(source).toContain('Console entries are captured verbatim');
    expect(source).toContain(RUNTIME_WARNING_LITERAL);
  });

  it('parses bounded CLI options and rejects malformed flags', () => {
    expect(probe.parseArgs(['--help', '--headed', '--url=https://example', '--duration-ms', '1', '--extension', 'ext'])).toMatchObject({
      help: true, headed: true, url: 'https://example', durationMs: 250, extensionPaths: ['ext'],
    });
    expect(() => probe.parseArgs(['--duration-ms'])).toThrow('requires a value');
    expect(() => probe.parseArgs(['--unknown', 'x'])).toThrow('unknown option');
  });

  it('discovers one version level of extensions and keeps invalid candidates visible', () => {
    const root = mkdtempSync(resolve(tmpdir(), 'ridge-runtime-attribution-test-'));
    const direct = resolve(root, 'direct');
    const versioned = resolve(root, 'versioned', '1.0');
    mkdirSync(direct, { recursive: true });
    mkdirSync(versioned, { recursive: true });
    writeFileSync(resolve(direct, 'manifest.json'), '{}');
    writeFileSync(resolve(versioned, 'manifest.json'), '{}');
    try {
      const found = probe.discoverExtensions({ extensionPaths: [direct, resolve(root, 'missing')], extensionsRoot: root });
      expect(found).toEqual(expect.arrayContaining([direct, versioned, resolve(root, 'missing')]));
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it('classifies clean baselines, extension load failures, and warning candidates', () => {
    expect(probe.classifyAttribution([])).toMatchObject({ status: 'missing-clean-baseline', ok: false });
    expect(probe.classifyAttribution([{ kind: 'clean-profile', runtimeLastErrorCount: 1 }])).toMatchObject({ status: 'warning-in-clean-profile', ok: false });
    expect(probe.classifyAttribution([
      { kind: 'clean-profile', runtimeLastErrorCount: 0 },
      { kind: 'extension', extensionLoaded: false, runtimeLastErrorCount: 0, extensionPath: 'bad' },
    ])).toMatchObject({ status: 'extension-load-unverified', attributionComplete: false });
    expect(probe.classifyAttribution([
      { kind: 'clean-profile', runtimeLastErrorCount: 0 },
      { kind: 'extension', extensionLoaded: true, runtimeLastErrorCount: 1, extensionPath: 'candidate' },
    ])).toMatchObject({ status: 'third-party-extension-candidate', candidates: ['candidate'] });
    expect(probe.classifyAttribution([
      { kind: 'clean-profile', runtimeLastErrorCount: 0 },
      { kind: 'extension', extensionLoaded: true, runtimeLastErrorCount: 0 },
    ])).toMatchObject({ status: 'no-warning-in-clean-or-tested-extensions', ok: true });
  });
});

const RUNTIME_WARNING_LITERAL =
  'Unchecked runtime.lastError: A listener indicated an asynchronous response by returning true, but the message channel closed before a response was received';
