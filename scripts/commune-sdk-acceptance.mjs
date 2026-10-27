// Independent official A2A SDK -> real Kernel/Hub -> an owned existing ACP process.
// This verifies protocol interoperability, never external-model or WAN behavior.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
const root=fs.mkdtempSync(path.join(os.tmpdir(),'ridge-sdk-commune-'));
const output=path.resolve(process.env.RIDGE_COMMUNE_EVIDENCE_DIR ?? 'artifacts/commune085','sdk.json');
const fixture=fs.readdirSync('target/debug/deps').filter(name=>/^commune_registration-[a-f0-9]+\.exe$/.test(name)).map(name=>path.resolve('target/debug/deps',name)).sort((a,b)=>fs.statSync(b).mtimeMs-fs.statSync(a).mtimeMs)[0];
assert.ok(fixture,'build commune_registration first');
const kernel=spawn(path.resolve('target/debug/ridge-kernel.exe'),[],{windowsHide:true,stdio:'ignore',env:{...process.env,RIDGE_KERNEL_DATA_DIR:root,RIDGE_AUTH_FILE:path.join(root,'auth.json')}});
let endpoint;
const evidence={scope:'Official independent SDK + actual Kernel/Hub + owned existing ACP fixture; external Agent and WAN unverified',result:'RUNNING'};
const until=async check=>{const end=Date.now()+20000;while(Date.now()<end){if(await check())return;await new Promise(resolve=>setTimeout(resolve,25));}throw Error('owned SDK peer readiness timeout');};
const call=async(method,url,body)=>{const response=await fetch(`http://127.0.0.1:${endpoint.port}${url}`,{method,headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},body:body?JSON.stringify(body):undefined});assert.ok(response.ok);return response.json();};
try {
  await until(()=>{if(!fs.existsSync(path.join(root,'kernel.json')))return false;endpoint=JSON.parse(fs.readFileSync(path.join(root,'kernel.json')));return endpoint.pid===kernel.pid;});
  const workspaceId=(await call('POST','/v1/domain/workspaces')).workspace_id,paneId=crypto.randomUUID();
  const descriptor=path.join(root,'peer.json');
  await call('POST','/v1/domain/ptys',{workspace_id:workspaceId,pty_id:paneId,program:fixture,args:['--exact','existing_protocol_endpoint_fixture','--ignored'],cwd:root,env:{RIDGE_COMMUNE_FIXTURE:descriptor},role:'shell',cols:80,rows:24});
  await until(()=>fs.existsSync(descriptor));const peer=JSON.parse(fs.readFileSync(descriptor));
  const registered=await call('POST','/v1/domain/commune/register',{workspaceId,paneId,process:peer.process,endpointOwner:peer.process,endpoint:peer.endpoint,sessionId:peer.sessionId,cwd:root});assert.equal(registered.ok,true);
  const config=path.join(root,'sdk-config.json');fs.writeFileSync(config,JSON.stringify({baseUrl:`http://127.0.0.1:${endpoint.port}`,token:endpoint.token,message:'sdk-existing-turn',cancelMessage:'wait-for-cancel',peerType:'owned-existing-ACP-fixture'}));
  const python=process.env.RIDGE_A2A_PYTHON ?? path.resolve('artifacts/commune085/a2a-sdk-env/Scripts/python.exe');
  const result=spawnSync(python,['scripts/commune-a2a-sdk-acceptance.py',config],{windowsHide:true,encoding:'utf8',timeout:60000});
  assert.equal(result.status,0,`${result.stdout}\n${result.stderr}`);
  evidence.sdk=JSON.parse(fs.readFileSync(path.join(root,'a2a-sdk-result.json')));evidence.result='PASS';
} catch(error) {evidence.result='FAIL';evidence.error=String(error);process.exitCode=1;}
finally {
  if(endpoint?.pid===kernel.pid)await call('POST','/v1/shutdown').catch(()=>{});
  fs.mkdirSync(path.dirname(output),{recursive:true});fs.writeFileSync(output,JSON.stringify(evidence,null,2));
  console.log(JSON.stringify({result:evidence.result,evidence:output}));
}
