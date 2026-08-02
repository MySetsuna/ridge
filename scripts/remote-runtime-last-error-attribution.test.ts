import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

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
});

const RUNTIME_WARNING_LITERAL =
  'Unchecked runtime.lastError: A listener indicated an asynchronous response by returning true, but the message channel closed before a response was received';
