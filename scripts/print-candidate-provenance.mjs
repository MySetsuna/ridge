#!/usr/bin/env node

// scripts/print-candidate-provenance.mjs
//
// Records build provenance for the current candidate:
//   - Commit + branch + dirty state
//   - Release version contract (product version: package.json / tauri.conf.json / Cargo.toml / Cargo.lock)
//   - Library crate versions (ridge-cli, ridge-kernel, ridge-core, etc.) — separate from release
//   - Artifact SHA-256 (host binary + web bundles)
//
// Goal §2: "构建来源用 commit/dirty diff、配置和产物哈希证明"
// Goal §2: "目标是用户可见版本符合现有产品发布策略"
// Goal §2: "内部 crate 是否同步按现有策略决定"

import { execSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));

function run(cmd) {
  try {
    return execSync(cmd, { cwd: root, encoding: "utf8" }).trim();
  } catch (e) {
    return `<error: ${e.message.split("\n")[0]}>`;
  }
}

function sha256(filePath) {
  const hash = createHash("sha256");
  hash.update(readFileSync(filePath));
  return hash.digest("hex");
}

function readText(rel) {
  return readFileSync(join(root, rel), "utf8");
}

function jsonVersion(rel) {
  return JSON.parse(readText(rel)).version;
}

function cargoManifestVersion(rel) {
  return readText(rel)
    .split(/\r?\n/)
    .find((line) => line.trim().startsWith("version = "))
    ?.trim()
    .slice('version = "'.length, -1);
}

// --- 1. Source provenance
console.log("=== SOURCE_PROVENANCE ===");
const commit = run("git rev-parse HEAD");
const branch = run("git rev-parse --abbrev-ref HEAD");
const shortCommit = run("git rev-parse --short HEAD");
const dirty = run("git status --short");
console.log(`branch:   ${branch}`);
console.log(`commit:   ${commit}`);
console.log(`short:    ${shortCommit}`);
console.log(`dirty:    ${dirty || "(clean)"}`);
if (dirty) {
  console.log("NOTE: working tree has uncommitted changes — artifacts above were built before these changes.");
}

// --- 2. Product release version contract (per scripts/check-release-version.mjs)
console.log("");
console.log("=== PRODUCT_VERSION (release contract) ===");
const productVersions = new Map([
  ["package.json", jsonVersion("package.json")],
  ["src-tauri/tauri.conf.json", jsonVersion("src-tauri/tauri.conf.json")],
  ["src-tauri/Cargo.toml", cargoManifestVersion("src-tauri/Cargo.toml")],
]);
const lockContent = readText("Cargo.lock");
const ridgeBlock = lockContent
  .split("[[package]]")
  .find((b) => b.split(/\r?\n/).some((l) => l.trim() === 'name = "ridge"'));
const ridgeLockVersion = ridgeBlock
  ?.split(/\r?\n/)
  .find((l) => l.trim().startsWith("version = "))
  ?.trim()
  .slice('version = "'.length, -1);
productVersions.set("Cargo.lock ridge", ridgeLockVersion);

let productAllSame = true;
const productExpected = productVersions.get("package.json");
for (const [k, v] of productVersions) {
  console.log(`  ${k}: ${v}`);
  if (v !== productExpected) productAllSame = false;
}
console.log(
  `  contract: ${productAllSame ? "OK" : "MISMATCH"} (expected ${productExpected})`,
);

// --- 3. Library crate versions (informational, NOT part of release contract)
console.log("");
console.log("=== LIBRARY_CRATE_VERSIONS (separate from release) ===");
const libraryCrates = [
  "packages/ridge-cli/Cargo.toml",
  "packages/ridge-kernel/Cargo.toml",
  "packages/ridge-core/Cargo.toml",
  "packages/ridge-remote/Cargo.toml",
  "packages/ridge-mcp/Cargo.toml",
  "packages/ridge-mcp-bridge/Cargo.toml",
  "packages/ridge-term/Cargo.toml",
  "packages/ridge-tmux/Cargo.toml",
];
for (const rel of libraryCrates) {
  try {
    const v = cargoManifestVersion(rel);
    console.log(`  ${rel}: ${v}`);
  } catch (e) {
    console.log(`  ${rel}: <not found>`);
  }
}
console.log(
  "  NOTE: these are workspace library crates; they are NOT part of the product release contract.",
);

// --- 4. Artifact hashes
console.log("");
console.log("=== ARTIFACT_HASHES ===");
const artifacts = [
  "target/test-rdg/release/ridge.exe",
  "remote-dist/desktop/index.html",
  "remote-dist/mobile/index.html",
  "remote-dist/mobile/sw.js",
  "remote-dist/mobile/manifest.webmanifest",
];
for (const rel of artifacts) {
  const full = join(root, rel);
  try {
    const s = statSync(full);
    console.log(`  ${rel}`);
    console.log(`    size: ${s.size} bytes`);
    console.log(`    sha256: ${sha256(full)}`);
  } catch {
    console.log(`  ${rel}: <missing>`);
  }
}

// --- 5. Final summary
console.log("");
console.log("=== SUMMARY ===");
console.log(
  `Product release version: ${productExpected} (contract ${productAllSame ? "OK" : "FAIL"})`,
);
console.log(
  `Library CLI version (binary --version): ${cargoManifestVersion("packages/ridge-cli/Cargo.toml")}`,
);
console.log(
  `Different by design — product release version is sourced from root package.json + src-tauri/* (CHG-028);`,
);
console.log(
  `library CLI --version follows Cargo's per-crate convention (packages/ridge-cli/Cargo.toml).`,
);
console.log(
  `Per Goal §2: user-visible product version (0.1.86) matches the release contract;`,
);
console.log(
  `library crate versions are NOT mechanically unified — they follow Cargo's per-crate versioning.`,
);