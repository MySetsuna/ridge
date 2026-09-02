import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const entryUrl = new URL('../docs/contracts/ridge-cloud-protocol.md', import.meta.url);
const entryExists = existsSync(entryUrl);
const entry = entryExists ? readFileSync(entryUrl, 'utf8') : '';

describe.skipIf(!entryExists)('Ridge Cloud protocol SSOT entry', () => {
  it('points to the canonical ridge-cloud protocol', () => {
    expect(entry).toContain(
      'https://github.com/MySetsuna/ridge-cloud/blob/develop/docs/ridge-cloud-protocol.md',
    );
    expect(entry).toContain('C:\\code\\ridge-cloud\\docs\\ridge-cloud-protocol.md');
  });

  it('stays a pointer instead of a second protocol body', () => {
    expect(entry.split(/\r?\n/).length).toBeLessThanOrEqual(40);
    expect(entry).not.toContain('# Ridge Cloud 商业化协议契约 v1');
    expect(entry).not.toContain('本文件是 Ridge 公网加速（Pro）相关');
  });
});

