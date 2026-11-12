import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const dir=path.resolve(process.argv[2]||'');
const rows=fs.readFileSync(path.join(dir,'samples.jsonl'),'utf8').trim().split('\n').filter(Boolean).map(JSON.parse);
const stat=values=>{const a=values.filter(Number.isFinite).sort((a,b)=>a-b);return{n:a.length,median:a.length?(a[Math.floor((a.length-1)/2)]+a[Math.floor(a.length/2)])/2:null,min:a[0]??null,max:a.at(-1)??null};};
const delta=(a,b)=>Number.isFinite(a)&&Number.isFinite(b)?a-b:null;
const ms=(a,b)=>{const d=delta(a,b);return d===null?null:d/1e6;};
const usToMs=(a,b)=>{const d=delta(a,b);return d===null?null:d/1000;};
function metrics(r){const s=r.sender,recv=r.receiver,t=recv?.transfers?.[0];if(!s||!t)return{};
 const cpu=x=>(x.cpu_user_us+x.cpu_system_us)/1000;
 return{share_ready_ms:ms(s.ticket_ready_ns,s.share_intent_ns),share_payload_ms:ms(t.first_payload_ns,s.share_intent_ns),
  share_return_ms:ms(t.receive_return_ns,s.share_intent_ns),share_commit_ms:ms(t.committed_ns,s.share_intent_ns),receive_payload_ms:ms(t.first_payload_ns,recv.receiver_intent_ns),
  receive_return_ms:ms(t.receive_return_ns,recv.receiver_intent_ns),
  prepare_rchar_bytes:delta(s.prepared.io.rchar,s.before.io.rchar),prepare_cpu_ms:delta(cpu(s.prepared),cpu(s.before)),
  sender_rchar_bytes:delta(s.after.io.rchar,s.before.io.rchar),sender_cpu_ms:delta(cpu(s.after),cpu(s.before)),
  sender_maxrss_kib:s.after.maxrss_kib,receiver_maxrss_kib:recv.after.maxrss_kib,
  relay_cpu_ms:usToMs(r.relay?.after?.cpu_usage_usec,r.relay?.before?.cpu_usage_usec),
  first_receive_cpu_ms:delta(cpu(t.after),cpu(t.before)),first_receive_write_bytes:delta(t.after.io.write_bytes,t.before.io.write_bytes),
  sampled_disk_logical_bytes:recv.disk.peak_logical_bytes,sampled_disk_allocated_bytes:recv.disk.peak_allocated_bytes,
  warm_receive_payload_ms:t.index===0&&recv.transfers.length>1?stat(recv.transfers.slice(1).map(t=>ms(t.first_payload_ns,t.receive_start_ns))).median:null,
  warm_receive_return_ms:t.index===0&&recv.transfers.length>1?stat(recv.transfers.slice(1).map(t=>ms(t.receive_return_ns,t.receive_start_ns))).median:null};}
const groups=[];
for(const id of [...new Set(rows.map(r=>r.case.id))].sort((a,b)=>a-b)){
 const g=rows.filter(r=>r.case.id===id),variants={},paired={};
 for(const v of ['legacy','stream']){const all=g.filter(r=>r.variant===v);assert.equal(new Set(all.map(r=>r.pair)).size,all.length);
  const good=all.filter(r=>r.success&&r.path_verified);const mapped=good.map(metrics);const keys=[...new Set(mapped.flatMap(Object.keys))];
  variants[v]={attempted:all.length,successful:all.filter(r=>r.success).length,path_verified:good.length,metrics:Object.fromEntries(keys.map(k=>[k,stat(mapped.map(m=>m[k]))]))};
  for(const r of good.filter(r=>r.variant==='stream'))assert.equal(r.sender.source_bytes_read_at_ready,0,'stream source was pre-read');
 }
 for(const n of g.filter(r=>r.variant==='stream'&&r.success&&r.path_verified)){
  const o=g.find(r=>r.variant==='legacy'&&r.pair===n.pair&&r.success&&r.path_verified);if(!o)continue;
  const old=metrics(o),next=metrics(n);for(const k of Object.keys(old)){const d=delta(old[k],next[k]);if(d!==null)(paired[k]??=[]).push(d);}
 }
 groups.push({case:g[0].case,variants,paired_savings:Object.fromEntries(Object.entries(paired).map(([k,v])=>[k,stat(v)]))});
}
const summary={attempts:rows.length,successes:rows.filter(r=>r.success).length,path_verified:rows.filter(r=>r.path_verified).length,
 transfers:rows.reduce((a,r)=>a+(r.receiver?.transfers?.length||0),0),groups};
fs.writeFileSync(path.join(dir,'summary.json'),JSON.stringify(summary,null,2)+'\n');
const sec=n=>Number.isFinite(n)?(n/1000).toFixed(3):'—';const mb=n=>Number.isFinite(n)?(n/1048576).toFixed(2):'—';
const lines=['# Source-stream engine A/B results','',`${summary.successes}/${summary.attempts} process-pair samples successful; ${summary.path_verified}/${summary.attempts} expected-path verified; ${summary.transfers} file receives recorded.`,'',
 'Medians below. Payload is an internal positive progress / first stream-frame proxy, NOT a packet-capture first byte. First-transfer end-to-end includes cold share preparation and 10ms ticket polling. No GUI, metadata advisory, device consent or Android provider staging is simulated.','',
 '| Case | Version | Valid / attempted | Share ready (s) | Share → payload (s) | Share → export/commit (s) | Share → API return (s) | Sender prepare rchar (MiB) | Sampled app+output peak disk (MiB) |',
 '| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |'];
for(const g of groups)for(const [v,s] of Object.entries(g.variants)){const m=k=>s.metrics[k]?.median;
 lines.push(`| ${g.case.id}. ${g.case.name} | ${v} | ${s.path_verified}/${s.attempted} | ${sec(m('share_ready_ms'))} | ${sec(m('share_payload_ms'))} | ${sec(m('share_commit_ms'))} | ${sec(m('share_return_ms'))} | ${mb(m('prepare_rchar_bytes'))} | ${mb(m('sampled_disk_logical_bytes'))} |`);}
lines.push('','## Repeated receives','',
 'Both versions reuse the same sender share. Stream also keeps its receiver endpoint/connection; legacy download creates a new receiver per call. Each process sample contributes the median of its two repeat receives. These are not new cold user-share measurements.','',
 '| Case | Version | Repeat receive → payload (s) | Repeat receive → return (s) |', '| --- | --- | ---: | ---: |');
for(const g of groups.filter(g=>g.case.count>1))for(const[v,s]of Object.entries(g.variants))lines.push(`| ${g.case.id} | ${v} | ${sec(s.metrics.warm_receive_payload_ms?.median)} | ${sec(s.metrics.warm_receive_return_ms?.median)} |`);
lines.push('','## Resource and interpretation limits','',
 '- Source preparation performs zero payload reads on the streaming path (explicit counter checked). Both paths incrementally hash the transmitted bytes; stream does not omit final integrity validation.',
 '- /proc/self/io rchar/write_bytes and getrusage CPU/RSS describe the entire instrumented Linux process, not isolated hardware counters or power consumption. Warm source page cache; rchar counts logical read syscalls, not mmap or physical disk reads.',
 '- Disk is the 10ms sampled sum of unique (device,inode) regular files in task-owned app temp plus output; source fixture excluded, tiny ticket/log files included, hard links counted once. Allocated blocks do not resolve shared reflink extents. Sampled peaks may miss short-lived duplicates and are not a hard upper bound.',
 '- Export/commit is the legacy receive-completed event versus successful stream no-clobber commit. Legacy does not explicitly fsync; stream syncs its file before commit, neither promises parent-directory crash durability. API return includes legacy endpoint close; stream retains endpoint/connection until explicit shutdown and returns after its completion receipt transport acknowledgement. Do not attribute all API-return savings to transfer itself.',
 '- Cold legacy baseline is the already optimized compatible engine, without app advisory metadata. Do not compare these numbers directly against the earlier app-orchestration benchmark.',
 '- Repeated receive byte-comparison occurs after each individual receive timer; aggregate process metrics can include comparison/sampling overhead. No p95, crash durability, production relay auth, phone GUI, unknown-length Android pipe or internet SLO claim.',
 '- Streaming is an explicit experimental single-regular-file API. No legacy ticket or installed client was automatically migrated; no retry/resume or disk spool is provided.','');
fs.writeFileSync(path.join(dir,'RESULTS.md'),lines.join('\n'));
console.log(JSON.stringify({attempts:summary.attempts,successes:summary.successes,path_verified:summary.path_verified,transfers:summary.transfers}));
