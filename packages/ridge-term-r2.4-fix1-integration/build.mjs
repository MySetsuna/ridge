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
//   node build.mjs           # release
//   node build.mjs --dev     # dev (faster compile, larger wasm)
//
// Or via npm script (added to Cargo project's package.json — none here,
// so users invoke this directly).

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const isDev = process.argv.includes('--dev');

console.log(`[ridge-term] ${isDev ? 'dev' : 'release'} build`);

// 1. Run wasm-pack. We don't pin its location — assume it's on PATH.
//    --target web: standard ESM output that works in Vite without plugins
//    --out-name ridge_term: matches the @ridge/term-wasm npm name we set below
const wasmPackArgs = [
	'build',
	'--target', 'web',
	'--out-dir', 'pkg',
	'--out-name', 'ridge_term',
	isDev ? '--dev' : '--release',
];

const wasmPackResult = spawnSync('wasm-pack', wasmPackArgs, {
	stdio: 'inherit',
	cwd: __dirname,
	shell: true, // shell:true makes Windows look up wasm-pack.cmd transparently
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
		'wasm-opt',
		['-Oz', '-o', optFile, wasmFile],
		{ stdio: 'pipe', shell: true },
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

console.log('[ridge-term] done.');
console.log('  next: cd ../ridge-app && pnpm add file:../ridge-term/pkg');
