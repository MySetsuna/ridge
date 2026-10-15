import {test,expect} from 'vitest';
import {existsSync} from 'node:fs';
import {distribution,assertRuntimePerformance,parseCliStages,RUNTIME_PERFORMANCE_SCENARIOS} from './lib/runtimePerformance.mjs';
test('nearest-rank percentiles retain the real maximum and valid sample count',()=>{
 const samples=Array.from({length:100},(_,index)=>100-index);
 expect(distribution(samples)).toMatchObject({count:100,p50:50,p95:95,p99:99,max:100});
 expect(distribution([1,2,3]).std).toBeCloseTo(Math.sqrt(2/3));
 expect(samples[0]).toBe(100);
 expect(distribution([])).toEqual({count:0,p50:null,p95:null,p99:null,max:null,std:null});
});
test.skipIf(!existsSync(new URL('../packages/ridge-term/pkg/ridge_term_bg.wasm',import.meta.url)))('real WASM reply observer handles control sequences without accepting escaped command echo',async()=>{
 const {createRuntimeReplyObserver}=await import('./lib/runtimeReplyObserver.mjs');
 const observer=await createRuntimeReplyObserver(24,80);
 try{
  observer.feed(Buffer.from('echo R^E^P^L^Y_00000210\r\n'));
  expect(observer.hasReply('00000210')).toBe(false);
  const output='REPLY_0000\x1b[31m0210\x1b[0m\r\n';
  expect(output.includes('REPLY_00000210')).toBe(false);
  for(const byte of Buffer.from(output))observer.feed(Uint8Array.of(byte));
  expect(observer.hasReply('00000210')).toBe(true);
  observer.feed(Buffer.from('LOAD\r\n'.repeat(1000)));
  expect(observer.cachedRows()).toBeLessThanOrEqual(256);
 }finally{observer.close();}
});
test('performance acceptance rejects missing samples, inactive load, frame errors and unverified minimization',()=>{
 const receipt={sampleCountPerClientPerScenario:2,groups:{},clientStages:{desktop:{errors:[]},remote:{errors:[]},cli:{enabled:false,truncated:false,errors:[]}},windowStates:[{minimized:true},{minimized:false}],history:{},transitions:{}};
 for(const kind of ['switch/desktop','switch/remote','history/cli','reconnect/remote','reconnect/cli','host-disconnected/runtime'])receipt.transitions[kind]={latencyMs:{count:2}};
 for(const client of ['desktop','remote'])receipt.history[client]={samples:Array.from({length:2},()=>({pages:1,rows:64,cachedRows:112,contentVerified:true}))};
 for(const scenario of RUNTIME_PERFORMANCE_SCENARIOS)for(const client of ['desktop','cli','remote']){
  receipt.groups[`${scenario}/${client}`]={latencyMs:{count:2},timeouts:0,failures:[],loadActive:[true,true]};
 }
 expect(()=>assertRuntimePerformance(receipt)).not.toThrow();
 const cases=[
  value=>delete value.groups['desktop-minimized/cli'],
  value=>value.groups['idle/desktop'].latencyMs.count--,
  value=>value.groups['idle/remote'].timeouts++,
  value=>value.groups['idle/cli'].failures.push('rejected input'),
  value=>value.groups['sustained-output/remote'].loadActive[0]=false,
  value=>value.groups['sustained-output/cli'].loadActive.pop(),
  value=>value.clientStages.desktop.errors.push('TERMINAL_REVISION_GAP'),
  value=>value.windowStates.pop(),
  value=>value.history.desktop.samples.pop(),
  value=>value.history.remote.samples[0].cachedRows=257,
  value=>value.history.desktop.samples[0].contentVerified=false,
  value=>value.history.remote.samples[0].pages=0,
  value=>value.clientStages.cli.errors.push('TERMINAL_REVISION_GAP'),
  value=>value.clientStages.cli.enabled=true,
  value=>value.clientStages.cli.truncated=true,
  value=>delete value.groups['switch/remote'],
  value=>value.transitions['reconnect/cli'].latencyMs.count--,
 ];
 for(const mutate of cases){const invalid=structuredClone(receipt);mutate(invalid);expect(()=>assertRuntimePerformance(invalid)).toThrow();}
});

test('CLI stage receipts bind the attested process, distinguish CPU stages and fence queues',()=>{
 const row=value=>`timestamp INFO RIDGE_CLI_STAGE ${JSON.stringify({event:'sample',pid:42,stage:'output_queue',durationMs:1,bytes:64,queueDepth:2,...value})}\n`;
 const receipt=parseCliStages(row({})+row({durationMs:3})+'partial RIDGE_CLI_STAGE {',true,42);
 expect(receipt.stages.output_queue).toEqual({latencyMs:{count:2,p50:1,p95:3,p99:3,max:3,std:1},queueDepthMax:2,bytesTotal:128});
 for(const value of [{pid:43},{queueDepth:33},{durationMs:-1},{stage:'fake'}])expect(()=>parseCliStages(row(value),true,42)).toThrow();
 expect(()=>parseCliStages(row({}),false,42)).toThrow();
 expect(parseCliStages('',false,42).samples).toBe(0);
 expect(parseCliStages(row({event:'truncated',limit:65536}),true,42).truncated).toBe(true);
});
