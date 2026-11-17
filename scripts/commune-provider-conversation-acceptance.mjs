// Real vendor ACP processes; only the model HTTP responses are controlled.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import http from 'node:http';
import {spawn} from 'node:child_process';
import {randomUUID,createHash} from 'node:crypto';
import {stripVTControlCharacters} from 'node:util';

const args=process.argv.slice(2);
assert.ok(args.length===0 || (args.length===2 && args[0]==='--profile' && ['claude','opencode'].includes(args[1])), 'Use --profile claude|opencode only for a scoped diagnostic');
const profiles=args.length ? [args[1]] : ['claude','opencode'];
const output=path.resolve(process.env.RIDGE_PROVIDER_EVIDENCE_DIR || 'artifacts/provider-conversations');
fs.mkdirSync(output,{recursive:true});
const target=path.resolve(process.env.CARGO_TARGET_DIR || 'target','debug');
const suffix=process.platform==='win32'?'.exe':'';
const inheritedPath=Object.entries(process.env).find(([key])=>key.toLowerCase()==='path')?.[1] || '';
const toolsPath=process.env.RIDGE_REAL_AGENT_PATH || inheritedPath;
const matrix={scope:'Real installed Claude ACP adapter/SDK and OpenCode CLI, private configuration and owned sessions; loopback Anthropic-compatible model only. Does not prove external models, WAN or takeover of a native user TUI.',grok:'SKIPPED / UNVERIFIED: explicit user waiver',profiles:[],status:'RUNNING'};
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(read,label,milliseconds=30000){
  const deadline=Date.now()+milliseconds;
  do{const value=await read();if(value)return value;await sleep(50);}while(Date.now()<deadline);
  throw Error(label);
}
async function stop(child){
  if(!child || child.exitCode!==null || child.signalCode!==null)return;
  const closed=new Promise(resolve=>child.once('close',resolve));
  if(process.platform==='win32')await new Promise(resolve=>spawn('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'}).once('close',resolve));
  else{try{process.kill(-child.pid,'SIGKILL');}catch{}}
  await Promise.race([closed,sleep(3000)]);
}
function executable(name){
  return toolsPath.split(path.delimiter).flatMap(directory=>(process.platform==='win32'?['.exe','.cmd','.bat','']:['']).map(extension=>path.join(directory,name+extension))).find(file=>fs.existsSync(file));
}

async function run(profile){
  const root=fs.mkdtempSync(path.join(os.tmpdir(),`ridge-real-${profile}-`));
  const cwd=path.join(root,'project'),data=path.join(root,'runtime'),config=path.join(root,'config');
  for(const folder of [cwd,data,config])fs.mkdirSync(folder);
  const report={profile,scope:matrix.scope,root,requests:[],checks:[],status:'RUNNING'};
  const state={phase:'initialization',held:false,aborted:false,marker:'',writePath:'',toolsSent:new Set(),writePaths:new Map()};
  const apiKey='owned-loopback-model-only';
  const responses=new Set(),children=[];
  let endpoint,viewer;
  const model=http.createServer(async(request,response)=>{
    responses.add(response);response.once('close',()=>responses.delete(response));
    try{
      let length=0;const chunks=[];
      for await(const chunk of request){length+=chunk.length;if(length>4*1024*1024){response.writeHead(413);response.end();return;}chunks.push(chunk);}
      const incoming=chunks.length?JSON.parse(Buffer.concat(chunks)):{};
      if(request.url.includes('count_tokens')){response.writeHead(200,{'content-type':'application/json'});response.end('{"input_tokens":100}');return;}
      if(request.method!=='POST' || !request.url.split('?')[0].endsWith('/messages')){response.writeHead(404);response.end('{}');return;}
      assert.ok(request.headers['x-api-key']===apiKey || request.headers.authorization===`Bearer ${apiKey}`,'Model request must use the explicit private fixture credential');
      const tools=incoming.tools || [];
      report.modelClient ||= request.headers['user-agent'];
      const writeTool=tools.find(tool=>/^(write|write_file)$/i.test(tool.name)) || tools.find(tool=>/write.*file/i.test(tool.name));
      const primary=Boolean(writeTool);
      const messages=JSON.stringify(incoming.messages);
      const userTurns=(incoming.messages || []).filter(message=>message.role==='user').map(message=>typeof message.content==='string'?message.content:
        (message.content || []).filter(block=>block.type==='text').map(block=>block.text).join('\n'));
      const inputMarker=[...userTurns.join('\n').matchAll(/Owned acceptance (RIDGE_REAL_[A-Z_]+)/g)].at(-1)?.[1];
      const requestPhase=inputMarker?.slice(`RIDGE_REAL_${profile.toUpperCase()}_`.length).toLowerCase();
      const inputCopies=inputMarker?userTurns.filter(text=>text.includes('Owned acceptance '+inputMarker)).length:0;
      if(primary)assert.equal(inputCopies,1,'The real Agent conversation contains a duplicate or missing user prompt');
      report.requests.push({path:request.url,phase:state.phase,requestPhase,inputMarker,inputCopies,maxTokens:incoming.max_tokens,retry:request.headers['x-stainless-retry-count'],model:incoming.model,primary,stream:incoming.stream,toolNames:tools.map(tool=>tool.name),
        markers:[...messages.matchAll(/RIDGE_REAL_[A-Z_]+/g)].map(match=>match[0]).slice(-12),messagesHash:createHash('sha256').update(messages).digest('hex')});
      assert.notEqual(state.phase,'initialization','Initialization must not start a model turn');
      if(primary && requestPhase==='cancel'){
        state.held=true;
        response.once('close',()=>{if(!response.writableEnded)state.aborted=true;});
        response.writeHead(200,{'content-type':'text/event-stream'});
        const message={id:'msg_'+randomUUID(),type:'message',role:'assistant',model:incoming.model,content:[],stop_reason:null,stop_sequence:null,usage:{input_tokens:100,output_tokens:0}};
        for(const value of [{type:'message_start',message},{type:'content_block_start',index:0,content_block:{type:'text',text:''}},
          {type:'content_block_delta',index:0,delta:{type:'text_delta',text:inputMarker+'_IN_PROGRESS'}}])response.write(`event: ${value.type}\ndata: ${JSON.stringify(value)}\n\n`);
        return;
      }
      let block={type:'text',text:inputMarker || state.marker};let stopReason='end_turn';
      if(primary && ['approve','deny'].includes(requestPhase) && !state.toolsSent.has(inputMarker)){
        state.toolsSent.add(inputMarker);
        const properties=writeTool.input_schema?.properties || {};
        const fileKey=['file_path','filePath','path'].find(key=>key in properties);
        const textKey=['content','text'].find(key=>key in properties);
        assert.ok(fileKey && textKey,`Unknown real ${profile} write schema`);
        block={type:'tool_use',id:'toolu_'+randomUUID(),name:writeTool.name,input:{[fileKey]:state.writePaths.get(inputMarker),[textKey]:inputMarker}};
        stopReason='tool_use';
      }
      const message={id:'msg_'+randomUUID(),type:'message',role:'assistant',model:incoming.model,content:[],stop_reason:null,stop_sequence:null,usage:{input_tokens:100,output_tokens:0}};
      response.writeHead(200,{'content-type':incoming.stream?'text/event-stream':'application/json','request-id':'owned-'+randomUUID()});
      if(!incoming.stream){response.end(JSON.stringify({...message,content:[block],stop_reason:stopReason,usage:{input_tokens:100,output_tokens:12}}));return;}
      const events=[['message_start',{type:'message_start',message}],
        ['content_block_start',{type:'content_block_start',index:0,content_block:block.type==='text'?{type:'text',text:''}:{...block,input:{}}}],
        ['content_block_delta',{type:'content_block_delta',index:0,delta:block.type==='text'?{type:'text_delta',text:block.text}:{type:'input_json_delta',partial_json:JSON.stringify(block.input)}}],
        ['content_block_stop',{type:'content_block_stop',index:0}],
        ['message_delta',{type:'message_delta',delta:{stop_reason:stopReason,stop_sequence:null},usage:{output_tokens:12}}],
        ['message_stop',{type:'message_stop'}]];
      for(const [event,value] of events)response.write(`event: ${event}\ndata: ${JSON.stringify(value)}\n\n`);
      response.end();
    }catch(error){report.fixtureError=String(error);if(!response.headersSent)response.writeHead(500,{'content-type':'application/json'});response.end('{"type":"error","error":{"type":"api_error","message":"owned fixture rejected request"}}');}
  });
  await new Promise(resolve=>model.listen(0,'127.0.0.1',resolve));
  const baseURL=`http://127.0.0.1:${model.address().port}`;
  const env=Object.fromEntries(Object.entries(process.env).filter(([key])=>!/^path$/i.test(key)&&!/^(ANTHROPIC_|CLAUDE_|OPENAI_|CODEX_|AWS_|GOOGLE_|GEMINI_|OPENCODE_)/i.test(key)));
  Object.assign(env,{[process.platform==='win32'?'Path':'PATH']:toolsPath,RIDGE_CDP_SHELL_PATH:toolsPath,
    CLAUDE_CONFIG_DIR:path.join(config,'claude'),ANTHROPIC_BASE_URL:baseURL,ANTHROPIC_API_KEY:apiKey,
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC:'1',DISABLE_TELEMETRY:'1',DISABLE_ERROR_REPORTING:'1',
    XDG_CONFIG_HOME:config,XDG_DATA_HOME:path.join(root,'data'),XDG_STATE_HOME:path.join(root,'state'),XDG_CACHE_HOME:path.join(root,'cache'),
    OPENCODE_CONFIG:path.join(config,'opencode.json'),OPENCODE_CONFIG_DIR:config,
    OPENCODE_DISABLE_AUTOUPDATE:'true',OPENCODE_DISABLE_MODELS_FETCH:'true',OPENCODE_DISABLE_PROJECT_CONFIG:'true',OPENCODE_DISABLE_CLAUDE_CODE:'true',
    NO_PROXY:'127.0.0.1,localhost',HTTP_PROXY:'http://127.0.0.1:1',HTTPS_PROXY:'http://127.0.0.1:1',
    RIDGE_KERNEL_DATA_DIR:data,RIDGE_KERNEL_FS_ROOT:root,RIDGE_WORKSPACE_SAVE_DIR:data,RIDGE_AUTH_FILE:path.join(data,'auth.json')});
  fs.mkdirSync(env.CLAUDE_CONFIG_DIR);
  // Ask explicitly in this private test configuration; the vendor's auto mode
  // can legitimately execute workspace writes without creating an ACP request.
  fs.writeFileSync(path.join(env.CLAUDE_CONFIG_DIR,'settings.json'),JSON.stringify({disableAutoMode:'disable',permissions:{defaultMode:'default'}}));
  report.permissionPolicy=profile==='claude'?'Private Manual/default mode with auto mode disabled':'Private ask policy';
  fs.writeFileSync(env.OPENCODE_CONFIG,JSON.stringify({enabled_providers:['anthropic'],model:'anthropic/claude-sonnet-4-5',small_model:'anthropic/claude-sonnet-4-5',
    provider:{anthropic:{options:{baseURL:baseURL+'/v1',apiKey}}},permission:{'*':'ask'},autoupdate:false,share:'disabled'}));
  function own(file,parameters){
    const child=spawn(file,parameters,{cwd,env,windowsHide:true,detached:process.platform!=='win32',stdio:['pipe','pipe','pipe']});
    children.push(child);child.output='';child.stdin.on('error',()=>{});
    child.on('error',error=>{child.output+='\n'+error.message;});
    for(const stream of [child.stdout,child.stderr])stream.on('data',bytes=>{child.output=(child.output+bytes.toString()).slice(-12000);});
    return child;
  }
  async function api(route,method='GET',body){
    const response=await fetch(`http://127.0.0.1:${endpoint.port}${route}`,{method,headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},
      body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(8000)});
    assert.equal(response.status,200);return response.json();
  }
  async function command(method,args,required=true){
    const reply=await api('/v1/domain/commands','POST',{method,args,native:true,roots:[],connectionId:randomUUID()});
    if(!required)return reply;assert.equal(reply.ok,true,`${method}: ${JSON.stringify(reply.error)}`);return reply.value;
  }
  const save=()=>fs.writeFileSync(path.join(output,`${profile}.json`),JSON.stringify(report,null,2));
  try{
    const tool=executable(profile==='claude'?'claude-agent-acp':'opencode');
    assert.ok(tool,`Required ${profile} executable/adapter is unavailable; install explicitly or supply RIDGE_REAL_AGENT_PATH`);
    report.executable=tool;
    for(const file of [path.join(path.dirname(tool),'../@agentclientprotocol/claude-agent-acp/package.json'),path.join(path.dirname(tool),'../package.json')]){
      if(fs.existsSync(file)){const value=JSON.parse(fs.readFileSync(file,'utf8'));report.package={name:value.name,version:value.version};
        if(profile==='claude'){
          const sdkFile=path.resolve(path.dirname(file),'../../@anthropic-ai/claude-agent-sdk/package.json');
          if(fs.existsSync(sdkFile)){const sdk=JSON.parse(fs.readFileSync(sdkFile,'utf8'));report.sdk={name:sdk.name,version:sdk.version};}
        }
        break;}
    }
    const kernel=own(path.join(target,'ridge-kernel'+suffix),[]);
    await until(()=>{assert.equal(kernel.exitCode,null,kernel.output);try{endpoint=JSON.parse(fs.readFileSync(path.join(data,'kernel.json'),'utf8'));return endpoint.pid===kernel.pid;}catch{return false;}},'Private Kernel readiness');
    viewer=own(path.join(target,'ridge'+suffix),['agent',profile,'--cwd',cwd]);
    const pane=await until(async()=>{assert.equal(viewer.exitCode,null,stripVTControlCharacters(viewer.output).replace(/ {4,}/g,' '));return(await api('/v1/domain/ptys')).ptys.find(row=>row.child_pid);},'Independent collaboration terminal');
    const candidate=await until(async()=>{assert.equal(viewer.exitCode,null,stripVTControlCharacters(viewer.output).replace(/ {4,}/g,' '));return(await command('get_teammate_topology',{workspaceId:pane.workspace_id})).discovered.find(row=>row.paneId===pane.pty_id&&row.connectionReady);},'Actual Agent session publication',60000);
    assert.equal(report.requests.length,0,'Read-only initialization started a model request');
    await command('set_teammate_groups',{groupScope:'host',expectedRevision:0,groups:[{id:'real-provider',name:'Real provider acceptance',color:'#60a5fa',memberAgentIds:[],createdAt:1}],enrollments:[{candidateRef:candidate.candidateRef,groupId:'real-provider'}]});
    const groupBefore=fs.readFileSync(path.join(data,'commune-groups.json'));
    const source={workspaceId:pane.workspace_id,paneId:pane.pty_id,candidateRef:candidate.candidateRef};
    const preview=await command('prepare_commune_connection',source);
    let identity=await command('register_commune_member',{...source,sessionRef:preview.sessionRef});
    const original={agentId:identity.agentId,sessionId:identity.sessionId,pid:pane.child_pid,start:pane.child_start_instance};
    report.identity=original;
    const published=path.join(data,'agent-connections');
    for(const file of fs.readdirSync(published).filter(file=>file.endsWith('.json'))){try{const value=JSON.parse(fs.readFileSync(path.join(published,file),'utf8'));if(value.agentName){report.agentName=value.agentName;report.agentProcess=value.process;}}catch{}}
    assert.equal(identity.sessionId,preview.sessionId);assert.deepEqual(fs.readFileSync(path.join(data,'commune-groups.json')),groupBefore);
    report.checks.push('Actual Agent initializes without a model prompt; terminal-first group, automatic candidate and exact session attach preserve identity');save();
    let targetBinding;
    const bind=()=>{targetBinding={workspaceId:pane.workspace_id,paneId:pane.pty_id,agentId:identity.agentId,generation:identity.generation,lease:identity.lease};};bind();
    async function dispatch(phase){
      state.phase=phase;state.marker=`RIDGE_REAL_${profile.toUpperCase()}_${phase.toUpperCase()}`;
      state.writePaths.set(state.marker,state.writePath);
      const request={...targetBinding,message:`Owned acceptance ${state.marker}. Return the marker only, or perform the exact requested owned test tool action.`,idempotency_key:randomUUID()};
      const delivery=await command('send_agent_message',request);
      assert.equal((await command('send_agent_message',request)).deliveryId,delivery.deliveryId,'Idempotent retry created another delivery');
      return delivery;
    }
    async function completed(delivery,rejectedByUser=false){
      return until(async()=>{
        assert.equal(viewer.exitCode,null,'Actual collaboration terminal exited');
        const value=await command('get_agent_message_delivery_status',{...targetBinding,deliveryId:delivery.deliveryId});
        if(rejectedByUser && value.execution?.state==='cancelled' && value.execution?.response?.stopReason==='cancelled')return value;
        if(['unknown','rejected','cancelled'].includes(value.execution?.state))throw Error(`Actual ${profile} turn failed: ${JSON.stringify(value.execution)}`);
        return value.execution?.state==='turn_ended'&&value;
      },`Actual ${profile} turn did not finish`,45000);
    }
    const text=await dispatch('text');await completed(text);
    assert.equal(report.requests.filter(row=>row.primary&&row.inputMarker===state.marker).length,1,'Exact retry started another model turn');
    assert.ok(JSON.stringify(await command('read_agent_recent_replies',{workspaceId:pane.workspace_id,limit:100})).includes(state.marker));
    report.checks.push('Real model response dispatches once through the original session and appears in Runtime history');save();
    for(const phase of ['deny','approve']){
      state.writePath=path.join(cwd,phase+'.txt');const delivery=await dispatch(phase);
      const permission=await until(async()=>(await command('list_hitl_pending_local',{})).find(row=>row.agentId===identity.agentId),`Real ${profile} permission absent`);
      assert.equal(permission.deliveryId,delivery.deliveryId,'Approval must belong to the original real turn');
      assert.equal(fs.existsSync(state.writePath),false,'Tool acted before the real permission decision');
      const kind=phase==='deny'?'reject_once':'allow_once';
      const option=permission.options?.find(row=>row.kind===kind);assert.ok(option,`Real permission lacks ${kind}`);
      assert.equal(await command('resolve_hitl_request',{id:permission.id,optionId:option.optionId}),true);
      const result=await completed(delivery,phase==='deny');
      (report.permissionOutcomes??=[]).push({phase,state:result.execution.state,stopReason:result.execution.response?.stopReason});
      if(phase==='deny')assert.equal(fs.existsSync(state.writePath),false,'Denied real tool wrote its file');
      else assert.equal(fs.readFileSync(state.writePath,'utf8'),state.marker);
    }
    report.checks.push('Actual Agent tool permissions reach Ridge; rejection writes nothing and explicit original-option approval permits only the owned file');save();
    const cancel=await dispatch('cancel');await until(()=>state.held,'Real cancellable model request absent');
    await until(async()=>JSON.stringify(await command('read_agent_recent_replies',{workspaceId:pane.workspace_id,limit:100})).includes(state.marker+'_IN_PROGRESS'),'Actual cancellation stream not observed');
    await command('cancel_agent_message',{...targetBinding,deliveryId:cancel.deliveryId});
    await until(async()=>{const s=await command('get_agent_message_delivery_status',{...targetBinding,deliveryId:cancel.deliveryId});return s.execution?.state==='cancelled'||s.execution?.response?.stopReason==='cancelled';},'Actual Agent cancellation absent');
    await until(()=>state.aborted,'Actual Agent did not abort the held model stream',10000);
    report.modelCancellationAborted=true;
    assert.equal(report.requests.filter(row=>row.primary&&row.inputMarker===state.marker).length,1,'Cancellation resent the prompt');
    assert.equal((await command('release_teammate_agent',targetBinding)).removed,true);
    identity=await command('register_commune_member',source);bind();
    assert.equal(identity.agentId,original.agentId);assert.equal(identity.sessionId,original.sessionId);
    await completed(await dispatch('after_reconnect'));
    const history=JSON.stringify(await command('read_agent_recent_replies',{workspaceId:pane.workspace_id,limit:100}));
    assert.ok(history.includes(`RIDGE_REAL_${profile.toUpperCase()}_TEXT`)&&history.includes(state.marker));
    assert.deepEqual(fs.readFileSync(path.join(data,'commune-groups.json')),groupBefore);
    const same=(await api('/v1/domain/ptys')).ptys.find(row=>row.pty_id===pane.pty_id);
    assert.equal(same.child_pid,original.pid);assert.equal(same.child_start_instance,original.start);
    report.checks.push('Real cancellation does not resend; reconnect retains exact session, terminal process, original group and observed history');save();
    viewer.stdin.write('/exit\n');await until(()=>viewer.exitCode!==null,'Explicit console exit did not complete',15000);
    assert.equal(viewer.exitCode,0);assert.deepEqual(fs.readdirSync(published),[]);
    assert.ok(Number.isSafeInteger(report.agentProcess?.pid),'Published real Agent process missing');
    await until(()=>{try{process.kill(report.agentProcess.pid,0);return false;}catch(error){if(error.code==='ESRCH')return true;throw error;}},'Actual Agent process survived explicit console exit',10000);
    assert.equal(report.fixtureError,undefined);
    report.checks.push('Explicit collaboration-terminal exit removes its published endpoint and terminates its owned Agent');report.status='PASS';
  }catch(error){
    report.status='FAIL';report.error=String(error.stack || error);
    report.processLogs=children.map(child=>({pid:child.pid,tail:stripVTControlCharacters(child.output).replace(/ {4,}/g,' ').slice(-2000)}));
    process.exitCode=1;
  }finally{
    try{if(endpoint)await api('/v1/shutdown','POST');}catch{}
    for(const child of children.reverse())await stop(child);
    for(const response of responses)response.destroy();
    await new Promise(resolve=>model.close(resolve));save();
  }
  console.log(JSON.stringify({profile,status:report.status,checks:report.checks,error:report.error}));return report;
}
for(const profile of profiles){matrix.profiles.push(await run(profile));fs.writeFileSync(path.join(output,'matrix.json'),JSON.stringify(matrix,null,2));}
matrix.status=matrix.profiles.every(row=>row.status==='PASS')?(profiles.length===2?'PASS':'PARTIAL_PASS'):'FAIL';
fs.writeFileSync(path.join(output,'matrix.json'),JSON.stringify(matrix,null,2));
