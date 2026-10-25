// Real Yjs ↔ Rust/Yrs and official MCP SDK interoperability against an isolated test server.
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import * as Y from 'yjs';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { rm } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
const probe=createServer();probe.listen(0,'127.0.0.1');await once(probe,'listening');const port=probe.address().port;await new Promise(r=>probe.close(r));
const db=`/tmp/minerva-protocol-${randomUUID()}.db`,base=`http://127.0.0.1:${port}`;
const child=spawn('./target/debug/minerva-server',[],{env:{...process.env,MINERVA_DATABASE:db,MINERVA_BIND:`127.0.0.1:${port}`,MINERVA_ADMIN_EMAIL:'protocol@test.local',MINERVA_ADMIN_PASSWORD:'protocol-test-password'},stdio:['ignore','pipe','pipe']});
let session,client;
async function api(path,body,method='POST') {const response=await fetch(base+'/api/v1/'+path,{method,headers:{'Content-Type':'application/json','X-Minerva-Client':'1',Authorization:'Bearer '+(session||'')},body:method==='GET'?undefined:JSON.stringify(body)});const result=await response.json();assert.equal(response.status,200,JSON.stringify(result));return result;}
const encode=v=>Buffer.from(v).toString('base64'),decode=v=>new Uint8Array(Buffer.from(v,'base64'));
try {
  await Promise.race([once(child.stdout,'data'),once(child,'exit').then(()=>{throw new Error('Test server exited before listening');}),new Promise((_,reject)=>{const t=setTimeout(()=>reject(new Error('Server startup timeout')),15000);t.unref();})]);
  session=(await api('login',{email:'protocol@test.local',password:'protocol-test-password'})).token;
  const boot=await api('bootstrap',null,'GET'),workspace=boot.workspaces[0],folder=boot.folders[0];
  const note=await api('notes',{folder_id:folder.id,title:'Shared protocol test'});
  const sync=body=>api(`notes/${note.id}/sync`,body);
  const initial=await sync({client_id:'one'});const a=new Y.Doc(),b=new Y.Doc();
  for(const doc of[a,b])Y.applyUpdate(doc,decode(initial.update));
  const bid=note.blocks[0].id;a.getText('block:'+bid).insert(0,'Alice 🌍');b.getText('block:'+bid).insert(0,'Bob ✨');
  await sync({client_id:'one',update:encode(Y.encodeStateAsUpdate(a))});
  const merged=await sync({client_id:'two',update:encode(Y.encodeStateAsUpdate(b))});
  for(const doc of[a,b])Y.applyUpdate(doc,decode(merged.update));
  assert.equal(a.getText('block:'+bid).toString(),b.getText('block:'+bid).toString());
  assert.match(merged.note.blocks[0].markdown,/Alice 🌍/);assert.match(merged.note.blocks[0].markdown,/Bob ✨/);
  const database=await api('databases',{name:'Protocol records',folder_id:folder.id});
  await api(`databases/${database.id}/rows`,{cells:{[database.columns[0].id]:'Read me',[database.columns[1].id]:'In progress'}});
  const credential=await api('mcp-credentials',{name:'SDK test',workspace_id:workspace.id,expires_days:1});
  client=new Client({name:'minerva-integration-test',version:'1.0'});
  await client.connect(new StreamableHTTPClientTransport(new URL(base+`/api/workspaces/${workspace.id}/mcp`),{requestInit:{headers:{Authorization:'Bearer '+credential.token}}}));
  const listed=await client.listTools();assert.equal(listed.tools.length,5);
  const search=await client.callTool({name:'doc_search',arguments:{query:'Alice'}});assert.equal(search.isError,false);assert.equal(JSON.parse(search.content[0].text).matches[0].document_id,note.id);
  const read=await client.callTool({name:'read_document',arguments:{document_id:note.id}});assert.equal(read.isError,false);assert.match(JSON.parse(read.content[0].text).blocks[0].markdown,/Bob ✨/);
  const records=await client.callTool({name:'query_database',arguments:{database_id:database.id,filter:{property_id:database.columns[1].id,value:'In progress'}}});assert.equal(records.isError,false);assert.equal(JSON.parse(records.content[0].text).records.length,1);
  const noRecords=await client.callTool({name:'query_database',arguments:{database_id:database.id,filter:{property_id:database.columns[1].id,value:'Done'}}});assert.equal(JSON.parse(noRecords.content[0].text).records.length,0);
  await api('mcp-credentials/'+credential.id,{},'DELETE');
  await assert.rejects(()=>client.listTools());
  console.log('PASS: Yjs/Yrs Unicode convergence, persistence projection, official MCP initialize/list/search/read/query/filter/revoke.');
} finally {
  if(client)await client.close().catch(()=>{});if(child.exitCode===null&&child.signalCode===null){const exited=once(child,'exit');child.kill('SIGINT');await exited.catch(()=>{});}
  for(const suffix of['','-wal','-shm'])await rm(db+suffix,{force:true});
}
