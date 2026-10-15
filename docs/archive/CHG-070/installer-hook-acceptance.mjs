// Execute the actual NSIS reservation macros in an owned temporary harness.
// No product registry, PATH, shortcuts, existing installation or user data writes.
import assert from 'node:assert/strict';
import {execFile,spawn} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdtemp,readFile,writeFile,mkdir} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {once} from 'node:events';
const execute=promisify(execFile);
const root=await mkdtemp(join(tmpdir(),'ridge-installer-hooks-'));
const compiler=join(process.env.LOCALAPPDATA,'tauri/NSIS/Bin/makensis.exe');
const installer=join(root,'harness.exe');
const source=join(root,'harness.nsi');
const data=join(root,'data');await mkdir(data);
await writeFile(source,`Unicode true
!include MUI2.nsh
!include FileFunc.nsh
!include LogicLib.nsh
!include "${resolve('src-tauri/installer-hooks.nsh')}"
Name "Ridge owned installer hook fixture"
OutFile "${installer}"
InstallDir "${join(root,'installed')}"
RequestExecutionLevel user
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
Section
  !insertmacro NSIS_HOOK_PREINSTALL
  nsExec::ExecToStack '"$PLUGINSDIR\\ridge-installer-runtime.exe" --installer-check'
  Pop $0
  Pop $1
  \${If} $0 == 0
    SetErrorLevel 3
    Quit
  \${EndIf}
  FileOpen $0 "$INSTDIR\\owned-marker.txt" w
  FileWrite $0 "RESERVED"
  FileClose $0
  !insertmacro RidgeRuntimeRelease
  nsExec::ExecToStack '"$PLUGINSDIR\\ridge-installer-runtime.exe" --installer-check'
  Pop $0
  Pop $1
  \${If} $0 != 0
    SetErrorLevel 4
    Quit
  \${EndIf}
SectionEnd
`);
// Escape template interpolation while preserving NSIS preprocessor expressions.
let child,endpoint;
const env={...process.env,RIDGE_KERNEL_DATA_DIR:data,RIDGE_KERNEL_FS_ROOT:root,
  RIDGE_WORKSPACE_SAVE_DIR:join(root,'snapshots'),RIDGE_AUTH_FILE:join(root,'auth.json')};
const delay=ms=>new Promise(done=>setTimeout(done,ms));
const report={root,scope:'actual NSIS hooks in owned harness; full product install/upgrade not exercised',checks:[]};
try {
  await execute(compiler,['/V2',source],{windowsHide:true,maxBuffer:1024*1024});
  await execute(installer,['/S'],{env,windowsHide:true,timeout:30000});
  assert.equal(await readFile(join(root,'installed/owned-marker.txt'),'utf8'),'RESERVED');
  report.checks.push('actual NSIS helper reserves both slots and acknowledges release before completion');
  child=spawn(resolve('target/debug/ridge-kernel.exe'),[],{env,windowsHide:true,stdio:'ignore'});
  const deadline=Date.now()+15000;
  while(Date.now()<deadline){
    assert.equal(child.exitCode,null);
    try{endpoint=JSON.parse(await readFile(join(data,'kernel.json'),'utf8'));if(endpoint.pid===child.pid)break;}catch{}
    await delay(50);
  }
  assert.equal(endpoint?.pid,child.pid);
  const before=await readFile(join(data,'kernel.json'));
  let rejected=false;
  try{await execute(installer,['/S',`/D=${join(root,'refused')}`],{env,windowsHide:true,timeout:30000});}
  catch(error){assert.equal(error.code,2);rejected=true;}
  assert.ok(rejected,'active Runtime must reject installation');
  assert.deepEqual(await readFile(join(data,'kernel.json')),before);
  await assert.rejects(readFile(join(root,'refused/owned-marker.txt')),error=>error.code==='ENOENT');
  assert.equal(child.exitCode,null);
  report.checks.push('active Runtime visibly refuses silent installation; registry and running child preserved');
  await writeFile(join(root,'result.json'),JSON.stringify(report,null,2));
  console.log(`PASS ${join(root,'result.json')}`);
} finally {
  if(child&&child.exitCode===null&&child.signalCode===null){
    if(endpoint){try{await fetch(`http://127.0.0.1:${endpoint.port}/v1/shutdown`,{method:'POST',headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},body:'{}',signal:AbortSignal.timeout(3000)});}catch{}}
    await Promise.race([once(child,'exit'),delay(5000)]);
    if(child.exitCode===null&&child.signalCode===null){child.kill();await once(child,'exit');}
  }
}
