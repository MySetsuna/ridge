// scripts/bench-latency-compare.mjs
// CHG-050 Phase C — read two bench JSON files (default-HTTP vs RIDGE_RTP1_KERNEL=1),
// surface per-scene delta with explicit NOT_RUN flagging.
//
// Usage:
//   node scripts/bench-latency-compare.mjs <label-a.json> <label-b.json> [<label-a> <label-b>]
//
// Output: stdout table + artifacts/release/latency/compare-<a>-vs-<b>.json

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { resolve, join } from "node:path";

const args = process.argv.slice(2);
if (args.length < 2) {
  console.error("usage: bench-latency-compare.mjs <a.json> <b.json> [labelA labelB]");
  process.exit(1);
}
const [pathA, pathB, labelA, labelB] = args;
const absA = resolve(pathA), absB = resolve(pathB);
if (!existsSync(absA)) { console.error(`missing: ${absA}`); process.exit(1); }
if (!existsSync(absB)) { console.error(`missing: ${absB}`); process.exit(1); }
const A = JSON.parse(readFileSync(absA, "utf8"));
const B = JSON.parse(readFileSync(absB, "utf8"));
const LA = labelA ?? A.label ?? "a";
const LB = labelB ?? B.label ?? "b";

const scenes = new Set([...Object.keys(A.scenes ?? {}), ...Object.keys(B.scenes ?? {})]);
const out = { a: LA, b: LB, scenes: {} };

console.log(`scene       | ${LA.padEnd(20)} | ${LB.padEnd(20)} | delta(p50)`);
console.log("-".repeat(72));
for (const s of scenes) {
  const a = A.scenes?.[s]?.stats;
  const b = B.scenes?.[s]?.stats;
  if (!a || !b) {
    console.log(`${s.padEnd(11)} | ${a ? "present" : "NOT_RUN"} | ${b ? "present" : "NOT_RUN"} | -`);
    out.scenes[s] = { a: a ?? null, b: b ?? null, status: (a && b) ? "OK" : "NOT_RUN" };
    continue;
  }
  const delta = (a.p50 !== null && b.p50 !== null) ? (b.p50 - a.p50) : null;
  const fmt = (n) => (n === null ? "NOT_RUN" : `${n}ms`);
  console.log(
    `${s.padEnd(11)} | ${fmt(a.p50).padEnd(20)} | ${fmt(b.p50).padEnd(20)} | ${delta === null ? "NOT_RUN" : (delta >= 0 ? "+" : "") + delta + "ms"}`,
  );
  out.scenes[s] = { a, b, deltaP50: delta, status: "OK" };
}

const cmpPath = join("artifacts/release/latency", `compare-${LA}-vs-${LB}.json`);
writeFileSync(cmpPath, JSON.stringify(out, null, 2));
console.log(`\nwrote ${cmpPath}`);