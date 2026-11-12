// Isolated DEV-only data-plane benchmark. No host tc/firewall/production changes.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
const root=path.resolve(process.argv[2]||'');
assert(root.startsWith('/home/luoji/code/dashbeam-source-stream-20261112.'));
const label=process.argv[3]||'main';assert(/^[a-z0-9-]+$/.test(label));
const repeats=Number(process.argv[4]||3);assert(Number.isInteger(repeats)&&repeats>=1&&repeats<=5);
const only=new Set((process.argv[5]||'').split(',').filter(Boolean).map(Number));
const windowMib=Number(process.env.DASHBEAM_BENCH_WINDOW_MIB||8);assert(Number.isInteger(windowMib)&&windowMib>=1&&windowMib<=16);
const cases=[
 {id:1,name:'cold-direct-small-lan',file:'one-mib.bin',relay:false,rtt:2,rate:1000,count:1},
 {id:2,name:'cold-direct-large-lan',file:'quarter-gib.bin',relay:false,rtt:2,rate:1000,count:1},
 {id:3,name:'cold-direct-small-wan',file:'one-mib.bin',relay:false,rtt:80,rate:50,count:1},
 {id:4,name:'cold-direct-large-wan',file:'sixtyfour-mib.bin',relay:false,rtt:80,rate:50,count:1},
 {id:5,name:'cold-relay-small-lan',file:'one-mib.bin',relay:true,rtt:2,rate:1000,count:1},
 {id:6,name:'cold-relay-small-wan',file:'one-mib.bin',relay:true,rtt:80,rate:50,count:1},
 {id:7,name:'cold-relay-large-wan',file:'sixtyfour-mib.bin',relay:true,rtt:80,rate:50,count:1},
 {id:8,name:'cold-relay-lossy',file:'sixteen-mib.bin',relay:true,rtt:150,rate:20,loss:0.5,count:1},
 {id:9,name:'repeat-relay-small-wan',file:'one-mib.bin',relay:true,rtt:80,rate:50,count:3},
 {id:10,name:'repeat-direct-small-wan',file:'one-mib.bin',relay:false,rtt:80,rate:50,count:3},
 // Opt-in regression probes: never silently expand the ten-condition matrix.
 {id:11,name:'high-bdp-direct',file:'sixtyfour-mib.bin',relay:false,rtt:80,rate:1000,count:1,extra:true},
 {id:12,name:'high-bdp-relay',file:'sixtyfour-mib.bin',relay:true,rtt:80,rate:1000,count:1,extra:true},
].filter(c=>only.size?only.has(c.id):!c.extra);assert(cases.length);
const experiment=`dbstream-1112-${label}-${crypto.randomBytes(3).toString('hex')}`;
const net=experiment,names={send:`${experiment}-s`,recv:`${experiment}-r`,relay:`${experiment}-x`};
const results=path.join(root,'results',label);fs.mkdirSync(results,{recursive:false});
const created=[];let networkCreated=false;
const image='dashbeam-benchmark:20261111';
const docker=(...args)=>execFileSync('docker',args,{encoding:'utf8',maxBuffer:4*1024*1024}).trim();
const exec=(role,...args)=>docker('exec',names[role],...args);
const info=name=>JSON.parse(docker('inspect',name))[0];
const ip=role=>info(names[role]).NetworkSettings.Networks[net].IPAddress;
const sha=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
function launch(args){const child=spawn('docker',args,{stdio:['ignore','pipe','pipe']});let stdout='',stderr='';
 child.stdout.on('data',b=>{if(stdout.length<8192)stdout+=b;});child.stderr.on('data',b=>{if(stderr.length<8192)stderr+=b;});
 return new Promise(resolve=>{child.on('error',e=>resolve({code:-1,error:e.message}));child.on('close',code=>resolve({code,stdout,stderr}));});}
function warm(file){const fd=fs.openSync(file,'r'),b=Buffer.alloc(1048576);try{while(fs.readSync(fd,b,0,b.length,null)>0){}}finally{fs.closeSync(fd);}}
function shape(c,seed){for(const [role,peer] of [['send','recv'],['recv','send']]){
 exec(role,'iptables','-F','OUTPUT');if(c.relay)exec(role,'iptables','-A','OUTPUT','-p','udp','-d',ip(peer),'-j','DROP');
 if(JSON.parse(exec(role,'tc','-j','qdisc','show','dev','eth0')).some(q=>q.kind==='netem'))exec(role,'tc','qdisc','del','dev','eth0','root');
 const args=['tc','qdisc','add','dev','eth0','root','netem','limit','10000','delay',`${c.rtt/2}ms`,'rate',`${c.rate}mbit`];
 if(c.loss)args.push('loss','random',`${c.loss}%`,'seed',String(seed+(role==='recv'?1:0)));exec(role,...args);
}}
const qdisc=()=>Object.fromEntries(['send','recv'].map(r=>[r,JSON.parse(exec(r,'tc','-s','-j','qdisc','show','dev','eth0'))]));
const relayResources=()=>({
 cpu_usage_usec:Number(exec('relay','cat','/sys/fs/cgroup/cpu.stat').match(/^usage_usec (\d+)$/m)?.[1]),
 memory_current_bytes:Number(exec('relay','cat','/sys/fs/cgroup/memory.current')),
 // Cumulative container high watermark, NOT a per-sample peak.
 memory_peak_bytes:Number(exec('relay','cat','/sys/fs/cgroup/memory.peak')),
});
const json=file=>{try{return JSON.parse(fs.readFileSync(file));}catch{return null;}};
async function sample(c,pair,variant){
 shape(c,5100+pair*2);const before=qdisc(),relayBefore=relayResources();
 const relative=path.join('samples',`${label}-c${c.id}-p${pair}-${variant}`),host=path.join(root,relative),shared=`/bench/${relative}`;
 fs.mkdirSync(host,{recursive:false,mode:0o700});warm(path.join(root,'fixtures',c.file));
 const common=['--variant',variant,'--shared',shared,'--source',`/fixtures/${c.file}`,'--count',String(c.count),'--relay',c.relay?'https://relay.dashbeam.test:3341':'disabled','--window-mib',String(windowMib)];
 const run=role=>launch(['exec','--user','1000:1000','-e','RUST_LOG=off','-e','SSL_CERT_FILE=/bench/tools/tls/ca.pem',names[role],
  'timeout','--signal=TERM','--kill-after=5','240','/bench/bin/source_stream_bench','--role',role,...common]);
 const [recvExit,sendExit]=await Promise.all([run('recv'),run('send')]);
 const sender=json(path.join(host,'sender.json')),receiver=json(path.join(host,'receiver.json'));
 const success=sendExit.code===0&&recvExit.code===0&&receiver?.bytes_match===true&&receiver.transfers.length===c.count;
 const paths=success&&receiver.transfers.every(t=>t.initial_path===(c.relay?'relay':'ip')&&t.final_path===(c.relay?'relay':'ip'));
 const row={case:c,pair,variant,success,path_verified:paths,sender,receiver,process_exit:{send:sendExit,recv:recvExit},qdisc_before:before,qdisc_after:qdisc(),relay:{before:relayBefore,after:relayResources()}};
 fs.appendFileSync(path.join(results,'samples.jsonl'),JSON.stringify(row)+'\n',{mode:0o600});
 console.log(JSON.stringify({case:c.id,pair,variant,success,path_verified:paths,share_ready_ms:sender?(sender.ticket_ready_ns-sender.share_intent_ns)/1e6:null,
  first_ms:receiver?.transfers[0]?.first_payload_ns&&sender?(receiver.transfers[0].first_payload_ns-sender.share_intent_ns)/1e6:null,
  return_ms:receiver&&sender?(receiver.transfers[0].receive_return_ns-sender.share_intent_ns)/1e6:null,
  source_read_at_ready:sender?.source_bytes_read_at_ready,receiver_peak:receiver?.disk}));
 // Exact task-owned synthetic exports/tickets only. Keep all success/failure metadata.
 for(const name of ['ticket','ticket.tmp']){const f=path.join(host,name);if(fs.existsSync(f))fs.unlinkSync(f);}
 for(const name of ['sender-temp','receiver-temp','receiver-output']){const f=path.join(host,name);if(fs.existsSync(f))fs.rmSync(f,{recursive:true});}
}
async function cleanup(){for(const name of created.reverse()){try{assert.equal(info(name).Config.Labels['org.dashbeam.experiment'],experiment);docker('rm','-f',name);}catch(e){console.error(`cleanup ${name}: ${e.message}`);}}
 if(networkCreated){try{docker('network','rm',net);}catch(e){console.error(e.message);}}}
try{
 assert(Number(execFileSync('df',['-B1','--output=avail',root],{encoding:'utf8'}).trim().split('\n').at(-1))>10*1024**3,'keep 10GiB free');
 docker('network','create','--internal','--label',`org.dashbeam.experiment=${experiment}`,net);networkCreated=true;
 for(const role of ['send','recv','relay']){
  const args=['run','-d','--name',names[role],'--network',net,'--cpus',role==='relay'?'1':'2','--memory','768m','--read-only','--tmpfs','/tmp:rw,nosuid,size=128m',
   '--cap-drop','ALL','--label',`org.dashbeam.experiment=${experiment}`,'-v',`${root}:/bench:rw`,'-v',`${root}/fixtures:/fixtures:ro`];
  if(role==='relay')args.push('--network-alias','relay.dashbeam.test');else args.push('--cap-add','NET_ADMIN','--cap-add','NET_RAW');
  args.push(image);docker(...args);created.push(names[role]);
 }
 docker('exec','-d','--user','1000:1000',names.relay,'/bench/bin/iroh-relay','--config-path','/bench/tools/relay.toml');await sleep(500);
 assert(exec('relay','ss','-lnt').includes(':3341'));assert(exec('relay','ss','-lnu').includes(':7842'));
 const environment={experiment,cases,repeats,kernel:exec('send','uname','-r'),image:info(names.send).Image,
  binary_sha256:sha(path.join(root,'bin/source_stream_bench')),scripts:Object.fromEntries(['run.mjs','source_stream_bench.rs'].map(f=>[f,sha(path.join(root,'tools',f))])),
  build:json(path.join(root,'results/build.json')),limits:{endpoint_cpus:2,relay_cpus:1,memory_bytes:768*1048576,workers:2,stream_transport_window_mib:windowMib},
  scope:'engine-only data plane; no metadata advisory/UI/SAF/consent; legacy current compatible engine versus experimental streaming; same source warm page cache; cold share per sample; repeat count reuses sender in BOTH variants, streaming additionally caches receiver connection; hash/copy CPU and IO included; byte comparison excluded from individual receive interval',
  network:JSON.parse(docker('network','inspect',net)).map(n=>({driver:n.Driver,internal:n.Internal,ipam:n.IPAM})),
  relay:{url:'https://relay.dashbeam.test:3341',authentication:'none',ca:'experiment only'},
  qdisc:'endpoint egress each RTT/2; seeded loss; peer UDP blocked in forced-relay cases; no host rules'};
 fs.writeFileSync(path.join(results,'environment.json'),JSON.stringify(environment,null,2)+'\n');
 for(const c of cases){shape(c,5000);fs.writeFileSync(path.join(results,`case-${c.id}-calibration.json`),JSON.stringify({case:c,ping:exec('send','ping','-n','-c','3','-i','0.2',ip('recv')),qdisc:qdisc(),rules:Object.fromEntries(['send','recv'].map(r=>[r,exec(r,'iptables','-S','OUTPUT')]))},null,2)+'\n');
  for(let pair=1;pair<=repeats;pair++){const order=(pair+c.id)%2?['stream','legacy']:['legacy','stream'];for(const v of order)await sample(c,pair,v);}
 }
}finally{await cleanup();}
