#!/usr/bin/env node
// Cross-platform build script for ridge-term wasm.
//
// Why Node instead of bash:
//   - build.sh fails silently on Windows (no `bash` in default PATH)
//   - The most important step is patching pkg/package.json to set
//     `name` to `@ridge/term-wasm`. If that step is skipped, downstream
//     `pnpm add file:...` ends up with the wrong package name.
//
// Usage:
//   node build.mjs                   # release, dual-backend (Canvas2D + WebGPU; default)
//   node build.mjs --dev             # dev (faster compile, larger wasm)
//   node build.mjs --no-webgpu       # release, Canvas2D-only (smaller wasm)
//   node build.mjs --dev --no-webgpu # dev + Canvas2D-only
//
// Note: `--webgpu` (legacy flag from round 4.5) is still accepted but is
// now a no-op — WebGPU ships in the default cargo feature set so the JS
// `RenderHandle.newWithWebgpuFirst` is always present, and TerminalManager
// runtime-detects the GPU adapter at attach time. To force a Canvas2D-only
// bundle (e.g. size-constrained), pass `--no-webgpu`.

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cargoTool } from '../../scripts/lib/toolPath.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const isDev = process.argv.includes('--dev');
// `--no-webgpu` strips the WebGPU backend by passing `--no-default-features`
// to cargo. Default builds include WebGPU because the user wants the GPU
// path on by default with runtime adapter detection (no build-time gate).
const noWebgpu = process.argv.includes('--no-webgpu');

console.log(
	`[ridge-term] ${isDev ? 'dev' : 'release'} build${noWebgpu ? ' (Canvas2D-only)' : ' (Canvas2D + WebGPU)'}`,
);

// 1. Run wasm-pack from the explicit Cargo bin directory (or an explicit
// RIDGE_WASM_PACK_PATH override).
//    --target web: standard ESM output that works in Vite without plugins
//    --out-name ridge_term: matches the @ridge/term-wasm npm name we set below
const wasmPackArgs = [
	'build',
	'--target', 'web',
	'--out-dir', 'pkg',
	'--out-name', 'ridge_term',
	isDev ? '--dev' : '--release',
];

// `wasm-pack build` forwards everything after `--` to cargo. The webgpu
// feature is in cargo's default feature set (Cargo.toml `[features]`),
// so we only need cargo-side flags when explicitly opting OUT.
if (noWebgpu) {
	wasmPackArgs.push('--', '--no-default-features');
}

const wasmPackResult = spawnSync(cargoTool('wasm-pack'), wasmPackArgs, {
	stdio: 'inherit',
	cwd: __dirname,
	shell: false,
});

if (wasmPackResult.status !== 0) {
	console.error('[ridge-term] wasm-pack failed');
	console.error('  install: cargo install wasm-pack');
	process.exit(wasmPackResult.status ?? 1);
}

// 2. Optional wasm-opt -Oz pass (release only). Skip silently if not installed.
if (!isDev) {
	const wasmFile = path.join(__dirname, 'pkg', 'ridge_term_bg.wasm');
	const optFile = path.join(__dirname, 'pkg', 'ridge_term_bg.opt.wasm');
	const optResult = spawnSync(
		cargoTool('wasm-opt'),
		['-Oz', '-o', optFile, wasmFile],
		{ stdio: 'pipe', shell: false },
	);
	if (optResult.status === 0) {
		fs.renameSync(optFile, wasmFile);
		console.log('[ridge-term] wasm-opt -Oz applied');
	} else {
		// Don't error — wasm-pack already ran a default wasm-opt -O pass.
		console.log('[ridge-term] wasm-opt not installed, skipping size optimization');
	}
}

// 3. **The critical step**: patch pkg/package.json. wasm-pack generates a
//    minimal package.json with `name: "ridge-term"` (the Cargo crate name).
//    We rename it to the scoped npm name `@ridge/term-wasm` so the
//    consumer-side imports match.
const pkgJsonPath = path.join(__dirname, 'pkg', 'package.json');
const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'));

pkg.name = '@ridge/term-wasm';
pkg.description = 'Ridge terminal: VT kernel + Canvas2D renderer (WASM)';
// sideEffects: false would let bundlers tree-shake everything; that
// breaks wasm-bindgen's init code which has top-level side effects.
// Be explicit about what's needed.
pkg.sideEffects = ['./ridge_term.js', './snippets/*'];
pkg.types = './ridge_term.d.ts';

fs.writeFileSync(pkgJsonPath, JSON.stringify(pkg, null, 2));
console.log(`[ridge-term] patched pkg/package.json → name = ${pkg.name}`);

// 4. Remove the auto-generated `pkg/.gitignore`. wasm-pack writes a
//    one-line `*` here so the build output is opt-out from VCS by
//    default — sensible if you publish to npm, but this project
//    consumes `pkg/` via `link:packages/ridge-term/pkg` from the root
//    package.json, so the directory MUST live in git to survive a
//    fresh clone. Re-deleting on every build keeps the workflow clean.
const pkgGitignorePath = path.join(__dirname, 'pkg', '.gitignore');
if (fs.existsSync(pkgGitignorePath)) {
	fs.unlinkSync(pkgGitignorePath);
	console.log('[ridge-term] removed wasm-pack-generated pkg/.gitignore (committed pkg/ workflow)');
}

console.log('[ridge-term] done.');
console.log('  next: cd ../ridge-app && pnpm add file:../ridge-term/pkg');
