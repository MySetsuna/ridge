// signaling/drift.test.ts — 漂移守卫（设计 §5，决策 B 续）。
//
// 若同级 `../ridge-signaling`（或 env RIDGE_SIGNALING_REPO）在场：逐字节比对 wind vendored
// `generated/` 与源 `bindings/`、vendored `fixtures/` 与源 `fixtures/signaling/`（含文件集合
// 一致），并校验 `SOURCE_REV` === 源 `git rev-parse HEAD`。任一不符即失败，提示运行
// `pnpm sync:signaling`。
//
// 同级缺席（CI / 未 checkout）→ 整组 skip：CI 零依赖，仍由 conformance.test.ts 对照 vendored
// fixtures 守住线形。

import { describe, it, expect } from 'vitest';
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
// signaling → cloud → remote → lib → src → <wind root>
const windRoot = resolve(here, '..', '..', '..', '..', '..');
const srcRepo = process.env.RIDGE_SIGNALING_REPO || resolve(windRoot, '..', 'ridge-signaling');

const VENDORED_GENERATED = join(here, 'generated');
const VENDORED_FIXTURES = join(here, 'fixtures');
const SRC_BINDINGS = join(srcRepo, 'bindings');
const SRC_FIXTURES = join(srcRepo, 'fixtures', 'signaling');

const siblingPresent = existsSync(srcRepo) && existsSync(SRC_BINDINGS) && existsSync(SRC_FIXTURES);

/** ts-rs 生成的 bindings 相对路径（与 sync 脚本一致）。 */
const BINDING_FILES = ['SignalMsg.ts', 'Role.ts', join('serde_json', 'JsonValue.ts')];

const RESYNC_HINT = '与同级 ridge-signaling 漂移——运行 `pnpm sync:signaling` 重新 vendor。';

/** 逐字节读（Buffer），用于 .equals 比对。 */
function readBytes(path: string): Buffer {
  return readFileSync(path);
}

describe.skipIf(!siblingPresent)('signaling drift guard（同级 ridge-signaling 在场）', () => {
  it('generated/ 与源 bindings/ 逐字节一致', () => {
    for (const rel of BINDING_FILES) {
      const vendored = join(VENDORED_GENERATED, rel);
      const source = join(SRC_BINDINGS, rel);
      expect(existsSync(vendored), `缺 vendored 文件 ${rel}：${RESYNC_HINT}`).toBe(true);
      expect(existsSync(source), `源缺 ${rel}（ridge-signaling 未生成？）`).toBe(true);
      expect(readBytes(vendored).equals(readBytes(source)), `${rel} 不一致：${RESYNC_HINT}`).toBe(
        true,
      );
    }
  });

  it('fixtures/ 与源 fixtures/signaling/ 文件集合一致且逐字节相同', () => {
    const vendoredNames = readdirSync(VENDORED_FIXTURES).filter((f) => f.endsWith('.json')).sort();
    const sourceNames = readdirSync(SRC_FIXTURES).filter((f) => f.endsWith('.json')).sort();
    expect(new Set(vendoredNames), `fixtures 文件集合不一致：${RESYNC_HINT}`).toEqual(
      new Set(sourceNames),
    );
    for (const name of sourceNames) {
      const a = readBytes(join(VENDORED_FIXTURES, name));
      const b = readBytes(join(SRC_FIXTURES, name));
      expect(a.equals(b), `fixture ${name} 不一致：${RESYNC_HINT}`).toBe(true);
    }
  });

  it('SOURCE_REV === 源 git rev-parse HEAD', () => {
    const recorded = readFileSync(join(here, 'SOURCE_REV'), 'utf8').trim();
    const head = execFileSync('git', ['rev-parse', 'HEAD'], {
      cwd: srcRepo,
      encoding: 'utf8',
    }).trim();
    expect(recorded, `SOURCE_REV 落后于源 HEAD：${RESYNC_HINT}`).toBe(head);
  });
});

// 同级缺席时留一条 skip 占位，让报告明确显示「漂移守卫已跳过（零依赖）」而非静默无测试。
describe.skipIf(siblingPresent)('signaling drift guard（同级缺席 → skip）', () => {
  it.skip('ridge-signaling 未 checkout：跳过逐字节比对，conformance 仍对照 vendored fixtures', () => {
    /* intentionally skipped */
  });
});
