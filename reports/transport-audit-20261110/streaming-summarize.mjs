// Rebuild the derived summary from final-source, sanitized raw JSONL only.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const base=path.dirname(fileURLToPath(import.meta.url));
const files=['streaming-direct-8k.jsonl','streaming-direct-64k.jsonl',
  'streaming-relay-first-65536.jsonl','streaming-relay-first-8192.jsonl'];
function stats(a) {
  a.sort((x,y)=>x-y);
  return {n:a.length,min:a[0],median:a[Math.floor(a.length/2)],max:a[a.length-1],
    interpretation:a.length===1?'single_observation_not_percentile':'three_samples_not_p95'};
}
const runs=[],groups=[];
for(const file of files) {
  const rows=fs.readFileSync(path.join(base,file),'utf8').trim().split('\n').map(JSON.parse);
  const metadata=rows.find(x=>x.kind==='run_metadata');
  const transfers=rows.filter(x=>x.kind==='transfer');
  const failures=rows.filter(x=>x.status==='error');
  runs.push({file,mode:metadata.mode,first_frame_bytes:metadata.first_frame_bytes,
    planned_transfers:metadata.planned_transfers,successful_transfers:transfers.length,
    failures,cleanup_pairs:rows.filter(x=>x.kind==='pair_cleanup').length,
    cleanup_ok:rows.filter(x=>x.kind==='pair_cleanup').every(x=>x.completed),
    all_transfer_verifications_passed:transfers.every(x=>x.status==='ok'
      &&x.peer_identity_verified_both_ends&&x.result.output_reread_verified
      &&x.result.receiver.manifest_verified&&x.result.receiver.fixture_hash_verified
      &&x.result.receiver.data_eof_verified
      &&x.result.sender.sender_bytes===x.bytes&&x.result.receiver.receiver_bytes===x.bytes)});
  for(const bytes of [1048576,67108864])
    for(const condition of ['cold_new_endpoint_pair','warm_same_connection']) {
      const a=transfers.filter(x=>x.bytes===bytes&&x.condition===condition);
      groups.push({file,mode:metadata.mode,bytes,condition,first_frame_bytes:metadata.first_frame_bytes,
        first_actual_byte_ms:stats(a.map(x=>x.result.receiver.first_actual_file_byte_from_shared_send_intent_ms)),
        first_frame_ack_ms:stats(a.map(x=>x.result.sender.first_frame_ack_from_send_intent_ms)),
        complete_ack_ms:stats(a.map(x=>x.result.sender.complete_ack_from_send_intent_ms)),
        source_first_read_ms:stats(a.map(x=>x.result.sender.source_first_read_ms)),
        file_open_stat_ms:stats(a.map(x=>x.result.sender.file_open_stat_ms))});
    }
}
const summary={kind:'derived_summary',scope:'final_source_only_same_DEV_autoaccept_no_application_aead',
  planned_transfers:runs.reduce((n,x)=>n+x.planned_transfers,0),
  successful_transfers:runs.reduce((n,x)=>n+x.successful_transfers,0),
  pair_failures:runs.reduce((n,x)=>n+x.failures.length,0),
  file_payload_bytes:groups.reduce((n,x)=>n+x.bytes*x.first_actual_byte_ms.n,0),runs,groups};
fs.writeFileSync(path.join(base,'streaming-summary.json'),JSON.stringify(summary,null,2)+'\n');
console.log(JSON.stringify({planned:summary.planned_transfers,success:summary.successful_transfers,
  pair_failures:summary.pair_failures,all_checks:runs.every(x=>x.cleanup_ok&&x.all_transfer_verifications_passed)}));
