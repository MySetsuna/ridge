import {mkdtemp,copyFile,readFile,writeFile,access} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve,dirname} from 'node:path';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import assert from 'node:assert/strict';
import {assertRuntimePerformance,performanceEnvironment,distribution} from './lib/runtimePerformance.mjs';

const root=await mkdtemp(join(tmpdir(),'ridge-performance-repeated-'));
console.log(`Comparison fixture: ${root}`);
async function run(name,settings){
 const child=spawn(process.execPath,['scripts/desktop-runtime-lifecycle-acceptance.mjs'],{windowsHide:true,
  env:{...process.env,RIDGE_RUNTIME_PERFORMANCE:'1',...settings},stdio:['ignore','pipe','pipe']});
 let output='';child.stdout.on('data',bytes=>output+=bytes);child.stderr.on('data',bytes=>output+=bytes);
 const [exitCode]=await once(child,'exit');await writeFile(join(root,`${name}.log`),output);
 assert.equal(exitCode,0,`owned performance ${name} run failed; see ${join(root,`${name}.log`)}`);
 const path=output.match(/PASS owned desktop lifecycle: (.+result\.json)/)?.[1]?.trim();assert.ok(path,'receipt path missing');
 const receipt=JSON.parse(await readFile(path,'utf8'));assertRuntimePerformance(receipt.performance);
 console.log(`PASS ${name}: ${path}`);return {receipt:path,performance:receipt.performance};
}
let baseline=process.env.RIDGE_PERFORMANCE_BASELINE_RECEIPT;
if(!baseline)baseline=(await run('seed',{RIDGE_PERFORMANCE_DIAGNOSTICS:'0'})).receipt;
const seed=JSON.parse(await readFile(baseline,'utf8')),seedRoot=dirname(baseline);
assertRuntimePerformance(seed.performance);
async function existingCopy(name,fallback){const candidate=join(seedRoot,name);try{await access(candidate);return candidate;}catch{return resolve(fallback);}}
const sources={kernel:join(seedRoot,'fixture-kernel.exe'),
 desktop:await existingCopy('fixture-desktop.exe','target/debug/ridge.exe'),
 cli:await existingCopy('fixture-cli.exe','target/070-cli-controller/debug/ridge.exe')};
const sourceHashes=(await performanceEnvironment(sources)).binarySha256;
const binaries={};
for(const [name,path] of Object.entries(sources)){
 assert.equal(sourceHashes[name],seed.performance.environment.binarySha256[name],`${name} differs from the seed receipt`);
 binaries[name]=join(root,`pinned-${name}.exe`);await copyFile(path,binaries[name]);
}
const assets={terminalWasm:resolve('packages/ridge-term/pkg/ridge_term_bg.wasm'),rendererWasm:resolve('packages/ridge-term-render/pkg/ridge_term_render_bg.wasm'),fontWasm:resolve('packages/ridge-term-font/pkg/ridge_term_font_bg.wasm')};
const expected=(await performanceEnvironment({...binaries,...assets})).binarySha256;
for(const [name,hash] of Object.entries(seed.performance.environment.binarySha256))assert.equal(expected[name],hash,`${name} differs from seed`);
// Reuse the valid seed; one matched opposite-mode run supplies a descriptive comparison.
const seedDiagnostics=seed.performance.diagnostics===true;
const results={off:[],on:[]},order=[seedDiagnostics,!seedDiagnostics];
results[seedDiagnostics?'on':'off'].push({receipt:baseline,performance:seed.performance});
for(const diagnostics of order.slice(1)){
 const group=diagnostics?'on':'off',name=`${group}-${results[group].length+1}`;
 const result=await run(name,{RIDGE_PERFORMANCE_DIAGNOSTICS:diagnostics?'1':'0',
  RIDGE_LIFECYCLE_KERNEL_SOURCE:binaries.kernel,RIDGE_LIFECYCLE_CLI_SOURCE:binaries.cli,
  RIDGE_LIFECYCLE_DESKTOP_SOURCE:binaries.desktop,RIDGE_LIFECYCLE_DESKTOP_CONFIG:join(seedRoot,'tauri-dev-cdp.config.json')});
 assert.deepEqual(result.performance.environment.binarySha256,expected,`${name} build artifacts changed`);
 results[group].push(result);
}
const aggregates={};
for(const group of ['off','on']){
 assert.equal(results[group].length,1);
 aggregates[group]=Object.fromEntries(Object.keys(results[group][0].performance.groups).map(key=>[key,{
  latencyMs:distribution(results[group].flatMap(run=>run.performance.groups[key].samples)),
  perRunP50Ms:distribution(results[group].map(run=>run.performance.groups[key].latencyMs.p50)),
 }]));
}
const comparison={completeChange:false,seed:baseline,order,artifactSha256:expected,results,aggregates,
 limitations:['one run per diagnostic setting, 32 samples per client/scenario; no repeated-run variance estimate; local debug build and loopback only','test injection, observation and uncontrolled system/GPU scheduling remain in measured latency','same executable and WASM artifacts; frontend served from the unchanged working tree','differences are descriptive, not causal proof of diagnostic overhead'],
 p50ChangeMs:Object.fromEntries(Object.keys(aggregates.off).map(key=>[key,aggregates.on[key].latencyMs.p50-aggregates.off[key].latencyMs.p50]))};
await writeFile(join(root,'comparison.json'),JSON.stringify(comparison,null,2));console.log(`Performance comparison: ${join(root,'comparison.json')}`);
