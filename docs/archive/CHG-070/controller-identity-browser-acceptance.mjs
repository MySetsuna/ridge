// Actual browser storage receipt; each case uses a fresh owned browser context.
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile,mkdtemp,writeFile} from 'node:fs/promises';
import {stripTypeScriptTypes} from 'node:module';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {chromium} from '@playwright/test';
const root=await mkdtemp(join(tmpdir(),'ridge-controller-browser-'));
const source=await readFile('src/lib/remote/cloud/legacyControllerIdentity.ts','utf8');
const module=stripTypeScriptTypes(source)+'\nwindow.readLegacyControllerSeed=readLegacyControllerSeed;';
const server=createServer((request,response)=>{
 response.setHeader('content-type',request.url==='/legacy.js'?'text/javascript':'text/html');
 response.end(request.url==='/legacy.js'?module:'<!doctype html><script type="module" src="/legacy.js"></script>');
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const browser=await chromium.launch({headless:true});
const report={completeChange:false,checks:[]};
try{
 for(const scenario of ['absence','idb','local','matching','conflict','malformed-idb','malformed-local','unreadable']){
  const context=await browser.newContext();const page=await context.newPage();
  try{
   await page.goto(`http://127.0.0.1:${server.address().port}`);
   await page.waitForFunction(()=>typeof window.readLegacyControllerSeed==='function');
   const result=await page.evaluate(async scenario=>{
    const bytes=new Uint8Array(32).fill(7);
    if(['local','matching','conflict','malformed-local'].includes(scenario))localStorage.setItem('ridge_ctrl_identity',btoa(String.fromCharCode(...(scenario==='malformed-local'?bytes.slice(1):bytes))));
    if(['idb','matching','conflict','malformed-idb'].includes(scenario))await new Promise((resolve,reject)=>{
     const open=indexedDB.open('ridge-cloud',1);
     open.onupgradeneeded=()=>open.result.createObjectStore('identity');
     open.onerror=()=>reject(open.error);
     open.onsuccess=()=>{
      const db=open.result,tx=db.transaction('identity','readwrite');
      tx.objectStore('identity').put(scenario==='malformed-idb'?bytes.slice(1):scenario==='conflict'?new Uint8Array(32).fill(8):bytes,'controller-ed25519-priv');
      tx.oncomplete=()=>{db.close();resolve();};tx.onerror=()=>reject(tx.error);
     };
    });
    const beforeLocal=localStorage.getItem('ridge_ctrl_identity');
    const beforeNames=(await indexedDB.databases()).map(database=>database.name).sort();
    let writes=0;const modes=[];
    for(const name of ['setItem','removeItem','clear']){const original=Storage.prototype[name];Storage.prototype[name]=function(...args){writes++;return original.apply(this,args);};}
    const transaction=IDBDatabase.prototype.transaction;
    IDBDatabase.prototype.transaction=function(stores,mode,...rest){modes.push(mode??'readonly');return transaction.call(this,stores,mode,...rest);};
    const databases=indexedDB.databases.bind(indexedDB);
    if(scenario==='unreadable')indexedDB.databases=()=>Promise.reject(new DOMException('fixture denied','SecurityError'));
    let value,error=false;
    try{value=await window.readLegacyControllerSeed();}catch{error=true;}
    const afterNames=(await databases()).map(database=>database.name).sort();
    const sameLocal=localStorage.getItem('ridge_ctrl_identity')===beforeLocal;
    let sameIdb=true;
    if(beforeNames.includes('ridge-cloud'))sameIdb=await new Promise((resolve,reject)=>{
     const open=indexedDB.open('ridge-cloud');open.onerror=()=>reject(open.error);
     open.onsuccess=()=>{const db=open.result,read=db.transaction('identity','readonly').objectStore('identity').get('controller-ed25519-priv');
      read.onsuccess=()=>{const expected=scenario==='malformed-idb'?bytes.slice(1):scenario==='conflict'?new Uint8Array(32).fill(8):bytes;resolve(read.result.length===expected.length&&read.result.every((byte,index)=>byte===expected[index]));db.close();};};
    });
    return {error,absent:value===null,expectedKey:Array.isArray(value)&&value.length===32&&value.every(byte=>byte===7),writes,modes,sameLocal,sameIdb,sameDatabases:JSON.stringify(beforeNames)===JSON.stringify(afterNames)};
   },scenario);
   assert.equal(result.error,['conflict','malformed-idb','malformed-local','unreadable'].includes(scenario),scenario);
   if(!result.error)assert.ok(scenario==='absence'?result.absent:result.expectedKey,scenario);
   assert.equal(result.writes,0);assert.ok(result.modes.every(mode=>mode==='readonly'));
   assert.ok(result.sameLocal&&result.sameIdb&&result.sameDatabases,scenario);
   report.checks.push(scenario);
  }finally{await context.close();}
 }
 await writeFile(join(root,'result.json'),JSON.stringify(report,null,2));
 console.log(`PASS read-only browser migration: ${join(root,'result.json')}`);
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
