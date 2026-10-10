// CHG-067: isolated, authenticated LAN route; controlled replies cannot match terminal-driver echo.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { performance } from 'node:perf_hooks';
import { spawn, spawnSync } from 'node:child_process';
import { chromium } from '@playwright/test';

if (process.argv.includes('--help')) {
  console.log('Usage: RIDGE_BENCH_LABEL=default RIDGE_BENCH_SAMPLES=20 RIDGE_BENCH_SCENES=idle,heavy,firstopen,switchback node scripts/bench-latency-4scenes.mjs\nAdditional scenes: history,reconnect,powershell,cmd,tui. Options: RIDGE_BIN, RIDGE_BENCH_HOST_PORT (default 5121), RIDGE_BENCH_TRACE=1 (client), RIDGE_BENCH_SERVER_TRACE=1, RIDGE_WATERFALL_TEST, RIDGE_RTP1_KERNEL. Own fixture only; no installed-service or system-trust changes.');
  process.exit(0);
}
const root = process.cwd();
const label = (process.env.RIDGE_BENCH_LABEL ?? 'default').replace(/[^a-zA-Z0-9_-]/g, '_').slice(0, 64);
const count = Number(process.env.RIDGE_BENCH_SAMPLES ?? '20');
if (!Number.isInteger(count) || count < 1 || count > 100) throw new Error('samples must be 1..100');
const scenes = (process.env.RIDGE_BENCH_SCENES ?? 'idle,heavy,firstopen,switchback').split(',');
if (scenes.some(scene => !['idle','heavy','firstopen','switchback','history','reconnect','powershell','cmd','tui'].includes(scene))) throw new Error('unknown scene');
const binary = path.resolve(process.env.RIDGE_BIN ?? 'target/debug/ridge.exe');
const port = Number(process.env.RIDGE_BENCH_HOST_PORT ?? '5121');
const state = fs.mkdtempSync(path.join(os.tmpdir(), 'ridge-diag-'));
const output = path.resolve(root, 'artifacts/release/latency');
fs.mkdirSync(output, { recursive: true });
const env = { ...process.env, RUST_LOG: 'info', RIDGE_AUTH_FILE: path.join(state, 'auth.json'), RIDGE_TEST_TLS_DIR: path.join(state, 'tls'), RIDGE_KERNEL_DATA_DIR: path.join(state, 'kernel'), RIDGE_PRINT_TOTP: '1', RIDGE_TEST_ALLOW_NON_BREAKAWAY: '1', RIDGE_REMOTE_HOST_REGISTRY: path.join(state, 'host.json'), RIDGE_REMOTE_UI_ROOT: path.join(root, 'remote-dist'), RIDGE_KERNEL_TRACE: process.env.RIDGE_BENCH_SERVER_TRACE==='1'?'1':'0', RIDGE_HOST_TRACE: process.env.RIDGE_BENCH_SERVER_TRACE==='1'?'1':'0' };
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function bounded(promise, timeout, label) {
  let timer;
  try { return await Promise.race([promise,new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error(label)),timeout);})]); }
  finally { clearTimeout(timer); }
}
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const git = args => spawnSync('git', args, { cwd: root, encoding: 'utf8', windowsHide: true }).stdout ?? '';
const result = { label, route: 'isolated authenticated localhost LAN WebSocket', clock: 'Node performance.now; browser spans separate; no cross-clock subtraction', binaryHash: hash(binary), sourceHead: git(['rev-parse','HEAD']).trim(), sourceDirtyFingerprint: crypto.createHash('sha256').update(git(['diff','--','src','packages','scripts','specs','changes'])).digest('hex'), traceEnabled: process.env.RIDGE_BENCH_TRACE === '1', kernelTransportFlag: process.env.RIDGE_RTP1_KERNEL ?? 'default', scenes: {}, coverage: { localhost: 'NOT_RUN', LAN_DEVICE: 'NOT_COVERED: no separate controller/device supplied', WAN_CELLULAR: 'NOT_COVERED: no authenticated WAN/cellular controller supplied', HOST_WEBVIEW_BACKGROUND: 'NOT_COVERED: CLI fixture has no host WebView' } };
let host, kernel, browser;
const childPanes = new Map();
let stderr = '';
let capturedCode = false;
result.serverTrace={samples:[],droppedSamples:0,clock:'receivedAtNodeMs is log receipt, not server event time; dt_us are same-process server spans'};
result.serverTrace.enabled=process.env.RIDGE_BENCH_SERVER_TRACE==='1';
function serverTrace(bytes, side) {
  if(!result.serverTrace.enabled) return;
  for(const line of bytes.toString().replace(/\x1b\[[0-9;]*m/g,'').split('\n')) {
    const kind=line.match(/\bev="?([a-z_]+)/)?.[1];
    if(!['dispatch_dequeue','write_in','pty_write','pty_byte_in','publish','lease_deliver','first_byte_or_timeout','recv_input','after_write','scroll','list_seq','attach','poll_in','poll_out','topo_poll'].includes(kind)) continue;
    const sample={side,kind,receivedAtNodeMs:performance.now()};
    const ok=line.match(/\bok=(true|false)/)?.[1]; if(ok!==undefined) sample.ok=ok==='true';
    for(const key of ['bytes','seq','input_sequence','dt_us','dt_from_recv_us','lock_wait_us','write_us','waited_ms','frames','queue_depth','queue_bytes','queue_wait_us','queue_lifetime_us','enqueued','dequeued']) { const value=line.match(new RegExp('\\b'+key+'=(\\d+(?:\\.\\d+)?)'))?.[1]; if(value!==undefined) sample[key]=Number(value); }
    for(const key of ['pty_id','pane_id']) { const value=line.match(new RegExp('\\b'+key+'="?([0-9a-fA-F-]{36})'))?.[1]; if(value) sample[key]=value; }
    const source=line.match(/\bsource_id="?([a-zA-Z0-9_.:-]{1,64})/)?.[1]; if(source && source!=='unavailable') sample.sourceId=source;
    const route=line.match(/\broute="?(direct|snapshot)/)?.[1]; if(route) sample.route=route;
    const lane=line.match(/\blane="?(input|control)/)?.[1]; if(lane) sample.lane=lane;
    result.serverTrace.samples.push(sample);
    if(result.serverTrace.samples.length>2048) { result.serverTrace.samples.shift(); result.serverTrace.droppedSamples++; }
  }
}
async function until(check, timeout = 15000) {
  const end = performance.now() + timeout;
  while (performance.now() < end) { const value = await bounded(Promise.resolve().then(check),Math.max(1,end-performance.now()),'bounded observation timeout'); if (value) return value; await delay(10); }
  throw new Error('bounded observation timeout');
}
function summary(samples, field = 'rttMs') {
  const values = samples.map(sample => sample[field]).filter(value => typeof value === 'number').sort((a,b) => a-b);
  const q = p => values.length ? values[Math.min(values.length - 1, Math.ceil(values.length * p) - 1)] : null;
  return { attempted: samples.length, successful: values.length, timeouts: samples.length-values.length, p50: q(.5), p95: q(.95), p99: q(.99), max: values.at(-1) ?? null };
}
function program(ready) {
  return "import sys,threading,time\ndef load(n):\n for i in range(n):\n  print('load_'+str(i),flush=True); time.sleep(.001)\nprint('"+ready+"',flush=True)\nfor line in sys.stdin:\n s=line.strip()\n if s.startswith('LOAD'): threading.Thread(target=load,args=(int(s.split('_')[1]) if '_' in s else 1000,),daemon=True).start()\n elif s=='EXIT': break\n else: print('reply_'+s,flush=True)";
}
async function active(page) {
  return page.evaluate(() => {
    const nodes = [...document.querySelectorAll('[data-rg-pane-id]')].filter(el=>el.getBoundingClientRect().width>0 && el.getBoundingClientRect().height>0);
    const node = nodes.find(el=>el.contains(document.activeElement)) ?? nodes.find(el => el.dataset.rgPaneActive === 'true') ?? nodes[0];
    const id = node?.dataset.rgPaneId;
    return window.__windE2E?.paneKeys().find(key => key === id || key.endsWith(':'+id)) ?? null;
  });
}
async function visible(page, key, text, timeout = 6000) {
  await until(() => page.evaluate(({key,text}) => window.__windE2E?.visibleText(key).join('\n').includes(text), {key,text}), timeout);
}
async function applicationEcho(page) {
  return bounded(page.evaluate(()=>window.__windE2E.applicationEcho()),6000,'bounded application RPC timeout');
}
async function kernelReplyPresent(key, expected) {
  const ep=JSON.parse(fs.readFileSync(path.join(env.RIDGE_KERNEL_DATA_DIR,'kernel.json'),'utf8'));
  if(kernel && ep.pid!==kernel.pid) throw new Error('owned Kernel identity changed');
  const id=key.includes(':')?key.slice(key.indexOf(':')+1):key;
  const reply=await fetch('http://127.0.0.1:'+ep.port+'/v1/domain/ptys/'+id+'?max_bytes=65536',{headers:{'x-ridge-kernel-token':ep.token},signal:AbortSignal.timeout(5000)});
  if(!reply.ok) return null;
  const value=await reply.json();
  return typeof value.data_b64==='string'?Buffer.from(value.data_b64,'base64').toString().includes(expected):null;
}
async function focusPane(page, key) {
  const id=key.includes(':')?key.slice(key.lastIndexOf(':')+1):key;
  const sink = page.locator('[data-rg-pane-id="'+id+'"]').filter({ visible: true }).first();
  await sink.click({ position: {x:60,y:40} });
}
async function typeLine(page, text, key = null) {
  await focusPane(page,key ?? await active(page));
  await bounded(page.keyboard.type(text),10000,'keyboard typing timeout');
  await bounded(page.keyboard.press('Enter'),10000,'keyboard submit timeout');
}
async function startChild(page, key) {
  const ready = 'ready_'+crypto.randomUUID().replaceAll('-','').slice(0,12);
  const encoded = Buffer.from(program(ready)).toString('base64');
  await typeLine(page, 'python -u -c "exec(__import__(\'base64\').b64decode(\''+encoded+'\'))"',key);
  await visible(page, key, ready);
  const workspace=await page.evaluate(()=>[...document.querySelectorAll('[data-ws-tab-id]')].find(tab=>tab.className.includes('bg-[var(--rg-accent)]/15'))?.getAttribute('data-ws-tab-id'));
  if(!workspace) throw new Error('controlled reply workspace ownership unavailable');
  childPanes.set(key,workspace);
}
async function newWorkspace(page) {
  const before=await page.evaluate(()=>window.__windE2E.paneKeys());
  await page.locator('button').filter({hasText:/^\+$/}).first().click();
  return until(async()=>{const keys=await page.evaluate(()=>window.__windE2E.paneKeys());return keys.find(key=>!before.includes(key));});
}
async function kernelMeasure() {
  const endpoint = JSON.parse(fs.readFileSync(path.join(env.RIDGE_KERNEL_DATA_DIR, 'kernel.json'), 'utf8'));
  const id = crypto.randomUUID();
  const call = async (method, suffix, body) => {
    const response = await fetch('http://127.0.0.1:'+endpoint.port+suffix, { method, headers: { 'x-ridge-kernel-token': endpoint.token, 'Content-Type':'application/json' }, body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(6000) });
    if (!response.ok) throw new Error('kernel HTTP status '+response.status);
    const value=await response.json();
    if(value.ok!==true) throw new Error('kernel domain operation rejected');
    return value;
  };
  const samples = [];
  try {
    await call('POST','/v1/domain/ptys',{pty_id:id,program:'python',args:['-u','-c',program('kernel_ready')],cwd:root,workspace_id:crypto.randomUUID(),role:'shell',cols:80,rows:24});
    const lease = await call('POST','/v1/domain/ptys/'+id+'/output');
    await delay(200);
    await call('GET','/v1/domain/ptys/'+id+'/output/'+lease.lease_id+'?timeout_ms=10&max_frames=64');
    for (let i=0;i<count;i++) {
      const nonce = 'K'+i+'_'+crypto.randomUUID().slice(0,8);
      const start = performance.now();
      await call('POST','/v1/domain/ptys/'+id+'/write',{data_b64:Buffer.from(nonce+'\r\n').toString('base64')});
      let text = '';
      try { await until(async () => { const reply=await call('GET','/v1/domain/ptys/'+id+'/output/'+lease.lease_id+'?timeout_ms=100&max_frames=64'); if(reply.kind==='lagged') throw new Error('kernel output lagged'); for(const frame of reply.frames??[]) text=(text+Buffer.from(frame.data_b64,'base64').toString()).slice(-4096); return text.includes('reply_'+nonce); },6000); samples.push({i,rttMs:performance.now()-start}); }
      catch { samples.push({i,rttMs:null}); }
    }
    return {stats:summary(samples),samples};
  } finally { await call('DELETE','/v1/domain/ptys/'+id).catch(()=>{}); }
}
try {
  kernel=spawn(binary,['--ridge-kernel-host'],{cwd:root,env,windowsHide:true,stdio:['ignore','ignore','pipe']});
  kernel.stderr.on('data',bytes=>serverTrace(bytes,'kernel'));
  await until(()=> {if(kernel.exitCode!==null)throw new Error('owned Kernel exited '+kernel.exitCode);return fs.existsSync(path.join(env.RIDGE_KERNEL_DATA_DIR,'kernel.json'));});
  host = spawn(binary,['host','--port',String(port)],{cwd:root,env,windowsHide:true,stdio:['ignore','ignore','pipe']});
  host.stderr.on('data', bytes => { if(!capturedCode) stderr=(stderr+bytes.toString()).slice(-65536); serverTrace(bytes,'host'); });
  const code = await until(() => { if(host.exitCode!==null) throw new Error('owned host exited '+host.exitCode); return stderr.match(/TOTP:\s*(\d{6})/)?.[1]; },30000);
  capturedCode=true;
  const hostRegistry=JSON.parse(fs.readFileSync(env.RIDGE_REMOTE_HOST_REGISTRY,'utf8'));
  result.host={pid:host.pid,port:hostRegistry.port,tls:hostRegistry.tls};
  const tls = path.join(state,'tls','cert.pem');
  await until(() => fs.existsSync(tls));
  const cert = new crypto.X509Certificate(fs.readFileSync(tls));
  const pin = crypto.createHash('sha256').update(cert.publicKey.export({type:'spki',format:'der'})).digest('base64');
  browser = await chromium.launch({headless:false,args:['--ignore-certificate-errors-spki-list='+pin]});
  const page = await browser.newPage({viewport:{width:1440,height:900}});
  page.setDefaultTimeout(10000); page.setDefaultNavigationTimeout(15000);
  // CDP evaluate/input methods can otherwise outlive Playwright's locator timeout.
  for(const [owner,name] of [[page,'evaluate'],[page,'reload'],[page,'screenshot'],[page.mouse,'wheel']]) {
    const call=owner[name].bind(owner); owner[name]=(...args)=>bounded(call(...args),20000,'bounded browser '+name+' timeout');
  }
  result.clientLoopProbeEnabled=process.env.RIDGE_BENCH_LOOP_TRACE==='1';
  await page.addInitScript(({trace,loop}) => {
    window.__RIDGE_E2E__=true; window.__RIDGE_REMOTE_PERF_TRACE=trace;
    if(loop) {
      window.__ridgeLoopProbe={gaps:[],longTasks:[]};
      let expected=performance.now()+50;
      setInterval(()=>{const at=performance.now();window.__ridgeLoopProbe.gaps.push({at,delayMs:Math.max(0,at-expected)});if(window.__ridgeLoopProbe.gaps.length>256)window.__ridgeLoopProbe.gaps.shift();expected=at+50;},50);
      if(PerformanceObserver.supportedEntryTypes.includes('longtask')) new PerformanceObserver(list=>{for(const e of list.getEntries()){window.__ridgeLoopProbe.longTasks.push({at:e.startTime,durationMs:e.duration});if(window.__ridgeLoopProbe.longTasks.length>128)window.__ridgeLoopProbe.longTasks.shift();}}).observe({type:'longtask',buffered:true});
    }
  },{trace:result.traceEnabled,loop:result.clientLoopProbeEnabled});
  const workers=new Set(); page.on('worker',worker=>workers.add(worker.url().startsWith('blob:')?'blob-worker':new URL(worker.url()).pathname));
  const resources = new Set(); page.on('response', response => { const url=new URL(response.url()); if(/\.(js|wasm|css)$/.test(url.pathname)) resources.add(url.pathname); });
  await page.goto('https://127.0.0.1:'+hostRegistry.port+'/?ui=desktop',{waitUntil:'domcontentloaded'});
  const input=page.locator('input[inputmode="numeric"]').first(); await input.waitFor({timeout:15000}); await input.fill(code); await input.press('Enter');
  stderr='';
  await page.waitForFunction(() => !!window.__windE2E?.paneKeys().length,{timeout:30000});
  result.frontend = {indexHash:hash(path.join(root,'remote-dist/desktop/index.html')),resources:[]};
  const initial=await active(page); if(!initial) throw new Error('no actual mounted pane key');
  await startChild(page,initial);
  result.kernelPty=await kernelMeasure();
  if(process.env.RIDGE_WATERFALL_TEST) {
    const test=spawn(path.resolve(process.env.RIDGE_WATERFALL_TEST),['waterfall_input_to_observed_output','--ignored','--nocapture'],{env:{...env,RIDGE_REQUIRE_WATERFALL:'1'},windowsHide:true,stdio:['ignore','ignore','pipe']});
    let lines=''; test.stderr.on('data',bytes=>lines=(lines+bytes.toString()).slice(-65536));
    const timeout=setTimeout(()=>test.kill(),120000);
    const exitCode=await new Promise((resolve,reject)=>{test.once('error',reject);test.once('exit',resolve);}); clearTimeout(timeout);
    result.rustClientWaterfall={exitCode,summary:lines.split('\n').filter(line=>line.includes('[waterfall]'))};
    if(exitCode!==0) throw new Error('required Rust-client waterfall failed');
  }
  for (const scene of scenes) {
    const samples=[];
    try {
      let key=await active(page);
      if(scene==='tui') throw new Error('no supplied authenticated TUI fixture; nvim/vim absent from PATH');
      // Shell scenes may precede these probes; each must own a known reply process.
      if(['idle','heavy','history','reconnect'].includes(scene) && !childPanes.has(key)) { key=await newWorkspace(page); await startChild(page,key); }
      if(scene==='powershell' || scene==='cmd') {
        key=await newWorkspace(page);
        await typeLine(page,scene==='powershell'?'pwsh -NoLogo -NoProfile -Command -':'cmd /D /Q');
        if(scene==='cmd') await until(()=>page.evaluate(key=>window.__windE2E.visibleText(key).some(line=>/^[a-zA-Z]:\\.*>\s*$/.test(line.trim())),key));
        const ready='S_READY_'+crypto.randomUUID().slice(0,8);
        await typeLine(page,scene==='powershell'?"Write-Output ('S_READY_'+'"+ready.slice(8)+"')":'echo S_READY_^'+ready.slice(8));
        await visible(page,key,ready);
      }
      if(scene==='heavy') await typeLine(page,'LOAD');
      if(scene==='history') { await typeLine(page,'LOAD_5000'); await until(()=>page.evaluate(key=>window.__windE2E.scrollbackLen(key)>0,key),15000); }
      if(scene==='switchback' && childPanes.size<2) { key=await newWorkspace(page); await startChild(page,key); }
      for(let i=0;i<count;i++) {
        let transitionMs;
        if(scene==='firstopen') { const transition=performance.now(); key=await newWorkspace(page); transitionMs=performance.now()-transition; await startChild(page,key); }
        if(scene==='switchback') { const owned=[...childPanes]; const [expected,workspace]=owned[i%2 ? owned.length-1 : 0]; const transition=performance.now(); await page.locator('[data-ws-tab-id="'+workspace+'"]').click(); key=await until(async()=>await active(page)===expected?expected:null); transitionMs=performance.now()-transition; }
        if(scene==='history') { await page.locator('[data-rg-pane-id]').filter({visible:true}).first().hover(); await page.mouse.wheel(0,-100000); }
        if(scene==='reconnect') { const transition=performance.now(); await page.reload({waitUntil:'domcontentloaded'}); await page.waitForFunction(()=>!!window.__windE2E?.paneKeys().length); key=await until(()=>active(page)); transitionMs=performance.now()-transition; }
        if(!['powershell','cmd'].includes(scene) && !childPanes.has(key)) throw new Error('selected pane has no verified controlled reply process');
        const nonce='U'+i+'_'+crypto.randomUUID().slice(0,8);
        await focusPane(page,key);
        await bounded(page.keyboard.type(scene==='powershell'?"Write-Output ('reply_'+'"+nonce+"')":scene==='cmd'?'echo reply_^'+nonce:nonce),10000,'keyboard typing timeout');
        const start=performance.now(); await bounded(page.keyboard.press('Enter'),10000,'keyboard submit timeout');
        try { await visible(page,key,'reply_'+nonce); samples.push({i,paneKey:key,rttMs:performance.now()-start,transitionMs}); }
        catch { samples.push({i,paneKey:key,rttMs:null,transitionMs,kernelReplyPresent:await kernelReplyPresent(key,'reply_'+nonce).catch(()=>null)}); }
      }
      const applicationSamples=[];
      for(let i=0;i<count;i++) { const start=performance.now();try { await applicationEcho(page); applicationSamples.push({i,rttMs:performance.now()-start}); } catch(error) { applicationSamples.push({i,rttMs:null,error:String(error.message)}); break; } }
      const stats=summary(samples);
      result.scenes[scene]={status:stats.timeouts?'MEASURED_WITH_TIMEOUTS':'MEASURED',stats,applicationStats:summary(applicationSamples),applicationSamples,...(samples.some(s=>s.transitionMs!==undefined)?{transitionStats:summary(samples,'transitionMs')} : {}),samples};
      result.scenes[scene].renderBackend=await page.evaluate(key=>window.__windE2E.backendName(key),key);
    } catch(error) { result.scenes[scene]={status:'NOT_COVERED',reason:String(error.message),samples}; }
    console.log('SCENE '+scene+' '+result.scenes[scene].status);
    result.scenes[scene].trace=await page.evaluate(()=>window.__RIDGE_REMOTE_PERF?.snapshot()??null).catch(()=>null);
    result.scenes[scene].clientLoop=await page.evaluate(()=>window.__ridgeLoopProbe??null).catch(()=>null);
    await page.evaluate(()=>window.__RIDGE_REMOTE_PERF?.reset()).catch(()=>{});
  }
  result.frontend.resources=[...resources].sort();
  result.frontend.observedDedicatedWorkers=[...workers];
  result.frontend.artifacts=result.frontend.resources.map(resource=> {
    const file=path.resolve(root,'remote-dist/desktop','.'+resource);
    return file.startsWith(path.resolve(root,'remote-dist/desktop')+path.sep) && fs.existsSync(file) ? {resource,sha256:hash(file)} : {resource,sha256:null};
  });
  result.frontend.activeServiceWorkers=await page.evaluate(async()=> 'serviceWorker' in navigator ? (await navigator.serviceWorker.getRegistrations()).map(r=>r.active ? new URL(r.active.scriptURL).pathname : null) : []).catch(()=>null);
  result.trace=await page.evaluate(()=>window.__RIDGE_REMOTE_PERF?.snapshot()??null).catch(()=>null);
  result.renderObservation='visibleText reads the parser/grid after feed; not physical presentation';
  result.applicationEcho='existing get_active_workspace_id RPC through authenticated business framing; control worker, not the latency-sensitive PTY input worker';
  await page.screenshot({path:path.join(output,label+'-actual-ui.png')}).catch(error=>{result.screenshotError=String(error.message);});
  result.coverage.localhost='MEASURED';
} catch(error) { result.error=String(error.message); result.coverage.localhost='NOT_COVERED: '+result.error; process.exitCode=1; }
finally {
  if(browser) await bounded(browser.close(),10000,'browser close timeout').catch(error=>{result.cleanupError=String(error.message);process.exitCode=1;});
  const endpointFile=path.join(env.RIDGE_KERNEL_DATA_DIR,'kernel.json');
  if(fs.existsSync(endpointFile)) {
    const ep=JSON.parse(fs.readFileSync(endpointFile,'utf8'));
    if(kernel && ep.pid!==kernel.pid) throw new Error('owned Kernel identity changed');
    try {
      const reply=await fetch('http://127.0.0.1:'+ep.port+'/v1/shutdown',{method:'POST',headers:{'x-ridge-kernel-token':ep.token},signal:AbortSignal.timeout(5000)});
      result.kernelShutdownHttp=reply.status;
      await until(()=>!fs.existsSync(endpointFile),10000);
    } catch(error) { result.cleanupError='owned Kernel graceful shutdown: '+String(error.message); process.exitCode=1; }
  }
  if(host?.pid && host.exitCode===null) host.kill();
  result.temporaryState=state;
  result.directoryCleanup='Retained: automated recursive directory deletion is blocked by local approval review.';
  fs.writeFileSync(path.join(output,label+'-bench.json'),JSON.stringify(result,null,2)+'\n');
  console.log('EVIDENCE '+path.join(output,label+'-bench.json'));
}
