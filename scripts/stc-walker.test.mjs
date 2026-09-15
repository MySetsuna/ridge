// scripts/stc-walker.test.mjs — regression tests for stc v0.1.1 walker + ignore.
//
// What this guards:
//   1. Generated dirs (target/, build/, .spectree/, remote-dist/, etc.) are
//      EXCLUDED from baseline.
//   2. Real source files (specs/, packages/*/src/, scripts/, etc.) ARE
//      included.
//   3. pnpm symlink-to-directory in nested node_modules (e.g.
//      packages/rg-split/node_modules/svelte) does NOT throw EISDIR.
//   4. Mutating a real source file changes exactly that file's hash.
//   5. Mutating a generated file does NOT change the baseline (because
//      it's excluded), so an unrelated generated-dir change wouldn't
//      trigger POLICY_E_UNAUTHORIZED_DIFF in lock-time.
//
// Run via: node --test scripts/stc-walker.test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, symlinkSync, rmSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createHash } from "node:crypto";

// Use the patched stc sources from the workspace's node_modules.
// pnpm virtual-store path encodes the patch hash; resolve via the
// @jackjiang18/spectree symlink in node_modules so the test follows
// whatever pnpm-currently-installs.
import { listFiles, fileHashes } from "../node_modules/@jackjiang18/spectree/dist/src/shared/index.js";
import { readIgnorePatterns } from "../node_modules/@jackjiang18/spectree/dist/src/compiler/index.js";

function sha(s) {
  return createHash("sha256").update(s).digest("hex");
}

function makeFixture() {
  const root = mkdtempSync(join(tmpdir(), "stc-walker-test-"));
  // Real source tree (must be INCLUDED)
  mkdirSync(join(root, "specs"), { recursive: true });
  writeFileSync(join(root, "specs/L1.md"), "# L1 spec\n");
  mkdirSync(join(root, "packages/foo/src"), { recursive: true });
  writeFileSync(join(root, "packages/foo/src/main.ts"), "export const x = 1;\n");
  writeFileSync(join(root, "packages/foo/package.json"), '{"name":"foo"}\n');

  // Generated / build dirs (must be EXCLUDED)
  mkdirSync(join(root, "target/release"), { recursive: true });
  writeFileSync(join(root, "target/release/big.bin"), "binary blob ".repeat(10000));
  mkdirSync(join(root, "build"), { recursive: true });
  writeFileSync(join(root, "build/output.js"), "// build artifact\n");
  mkdirSync(join(root, "remote-dist/mobile"), { recursive: true });
  writeFileSync(join(root, "remote-dist/mobile/sw.js"), "// service worker\n");
  mkdirSync(join(root, ".spectree"), { recursive: true });
  writeFileSync(join(root, ".spectree/config.json"), '{"version":1}\n');
  mkdirSync(join(root, ".git"), { recursive: true });
  writeFileSync(join(root, ".git/HEAD"), "ref: refs/heads/main\n");

  // Symlink-to-directory (must be skipped, NOT EISDIR). Point at a real
  // dir under our temp area.
  const target = join(root, "_symlink-target");
  mkdirSync(target, { recursive: true });
  writeFileSync(join(target, "inside.ts"), "// nested\n");
  mkdirSync(join(root, "packages/foo/node_modules"), { recursive: true });
  try {
    symlinkSync(target, join(root, "packages/foo/node_modules/dep"), "dir");
  } catch {
    // some platforms forbid dir symlinks; skip that branch of the test
  }

  // .stcignore
  writeFileSync(
    join(root, ".stcignore"),
    [
      "# project-local ignore",
      "build/",
      "remote-dist/",
      "target/",
      "target-*/",
      ".ridge/",
    ].join("\n"),
  );

  // .spectree/config.json pointing at .stcignore
  mkdirSync(join(root, ".spectree"), { recursive: true });
  writeFileSync(
    join(root, ".spectree/config.json"),
    JSON.stringify({ version: 1, ignoreFile: ".stcignore" }, null, 2),
  );

  return root;
}

test("readIgnorePatterns merges hardcoded default + .stcignore", () => {
  const root = makeFixture();
  const patterns = readIgnorePatterns(root);
  for (const must of [
    ".spectree",
    ".git",
    "node_modules",
    "dist",
    ".iteration",
    "build",
    "remote-dist",
    "target",
  ]) {
    assert.ok(patterns.includes(must), `expected ${must} in ignore patterns`);
  }
  // glob pattern preserved
  assert.ok(patterns.includes("target-*"), "expected target-* glob preserved");
});

test("listFiles excludes generated dirs and includes real source", () => {
  const root = makeFixture();
  const patterns = readIgnorePatterns(root);
  const files = listFiles(root, patterns);

  // Real source must be present
  assert.ok(files.includes("specs/L1.md"), "specs/L1.md must be in result");
  assert.ok(files.includes("packages/foo/src/main.ts"), "src must be in result");
  assert.ok(files.includes("packages/foo/package.json"), "package.json must be in result");

  // Generated / state must be excluded
  for (const must of [
    ".spectree/config.json",
    ".git/HEAD",
    "target/release/big.bin",
    "build/output.js",
    "remote-dist/mobile/sw.js",
  ]) {
    assert.ok(!files.includes(must), `${must} must NOT be in result`);
  }
});

test("symlink-to-directory does not throw EISDIR", () => {
  const root = makeFixture();
  const patterns = readIgnorePatterns(root);
  // If EISDIR wasn't handled, this throws or hangs.
  const files = listFiles(root, patterns);
  // The symlink target's "inside.ts" must NOT appear (the symlink is
  // intentionally skipped, NOT recursed into — the target isn't an
  // ignored prefix, but skipping symlinks-to-dir is the documented
  // policy because pnpm stores nested deps that way).
  assert.ok(!files.includes("packages/foo/node_modules/dep/inside.ts"),
    "symlink-to-dir contents must not leak into baseline");
});

test("mutating a generated file does NOT change baseline", () => {
  const root = makeFixture();
  const patterns = readIgnorePatterns(root);
  const before = fileHashes(root, patterns);

  // Mutate a generated file (inside target/, ignored)
  writeFileSync(join(root, "target/release/big.bin"), "completely different content");
  const after = fileHashes(root, patterns);

  // Baseline should be unchanged
  assert.deepEqual(after, before, "ignoring target/ means the mutation is invisible to baseline");
});

test("mutating a real source file changes exactly that file's hash", () => {
  const root = makeFixture();
  const patterns = readIgnorePatterns(root);
  const before = fileHashes(root, patterns);

  writeFileSync(join(root, "packages/foo/src/main.ts"), "export const x = 999;\n");
  const after = fileHashes(root, patterns);

  const changedPaths = Object.keys(after).filter((k) => before[k] !== after[k]);
  assert.deepEqual(changedPaths, ["packages/foo/src/main.ts"],
    `only the mutated file should change; got: ${changedPaths.join(",")}`);

  // Sanity: the new hash matches what we'd compute locally
  const newContent = readFileSync(join(root, "packages/foo/src/main.ts"), "utf8");
  assert.equal(after["packages/foo/src/main.ts"], sha(newContent));
});

test("baseline path set is bounded (does not include generated content)", () => {
  const root = makeFixture();
  const patterns = readIgnorePatterns(root);
  const before = fileHashes(root, patterns);

  // Write a 10MB generated file; baseline file count should NOT change.
  const blob = "x".repeat(10_000_000);
  writeFileSync(join(root, "build/big.txt"), blob);
  const after = fileHashes(root, patterns);
  assert.equal(Object.keys(after).length, Object.keys(before).length,
    "ignoring build/ means baseline file count is stable across generated-dir churn");
});
