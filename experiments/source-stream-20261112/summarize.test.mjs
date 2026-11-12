import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const script=fileURLToPath(new URL('./summarize.mjs',import.meta.url));
const p={io:{rchar:20,write_bytes:30},cpu_user_us:100,cpu_system_us:20,maxrss_kib:5000};
function row(variant,pair,success=true){return{case:{id:1,name:'synthetic',count:1},variant,pair,success,path_verified:success,
 sender:{share_intent_ns:0,ticket_ready_ns:100,source_bytes_read_at_ready:variant==='stream'?0:null,before:p,prepared:p,after:p},
 receiver:{receiver_intent_ns:200,transfers:[{index:0,first_payload_ns:300,receive_start_ns:200,receive_return_ns:500,before:p,after:p}],after:p,disk:{peak_logical_bytes:10,peak_allocated_bytes:4096}}};}
function run(rows,check){const dir=fs.mkdtempSync(path.join(os.tmpdir(),'dashbeam-stream-summary-'));
 try{fs.writeFileSync(path.join(dir,'samples.jsonl'),rows.map(JSON.stringify).join('\n')+'\n');
  const execute=()=>execFileSync(process.execPath,[script,dir],{stdio:'pipe'});
  check(execute,()=>JSON.parse(fs.readFileSync(path.join(dir,'summary.json'))));
 }finally{fs.rmSync(dir,{recursive:true});}}
test('retain failed attempts and compute savings only from valid pairs',()=>{
 run([row('legacy',1),row('stream',1),row('legacy',2,false),row('stream',2)],(exec,read)=>{exec();const s=read();
  assert.equal(s.attempts,4);assert.equal(s.successes,3);assert.equal(s.groups[0].variants.legacy.attempted,2);
  assert.equal(s.groups[0].paired_savings.share_payload_ms.n,1);});});
test('reject hidden source pre-read on streaming path',()=>{const r=row('stream',1);r.sender.source_bytes_read_at_ready=1;
 run([row('legacy',1),r],exec=>assert.throws(exec));});
test('reject duplicate variant/pair',()=>run([row('legacy',1),row('legacy',1)],exec=>assert.throws(exec)));
test('all failures produce no fabricated median',()=>run([row('legacy',1,false),row('stream',1,false)],(exec,read)=>{exec();
 assert.deepEqual(read().groups[0].variants.stream.metrics,{});assert.deepEqual(read().groups[0].paired_savings,{});}));
test('relay CPU uses microseconds and missing counters stay unknown',()=>{const r=row('stream',1);
 r.relay={before:{cpu_usage_usec:1000},after:{cpu_usage_usec:3500}};
 run([row('legacy',1),r],(exec,read)=>{exec();const v=read().groups[0].variants;
  assert.equal(v.stream.metrics.relay_cpu_ms.median,2.5);assert.equal(v.legacy.metrics.relay_cpu_ms.n,0);
  assert.equal(v.legacy.metrics.relay_cpu_ms.median,null);});});
