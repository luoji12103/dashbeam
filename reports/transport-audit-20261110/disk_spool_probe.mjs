// Isolated loopback experiment, Node >=18; run only on DEV.
// Real HTTP requests, real disk files, per-frame AEAD, bounded disk credit.
// Synthetic source, ephemeral credentials/key. No production configuration.
// This is NOT a production spool: no recovery journal, multi-user policy or TLS.
import http from 'node:http';
import fs from 'node:fs';
import fsp from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {once} from 'node:events';
import {setTimeout as pause} from 'node:timers/promises';

const FRAME=64*1024, MIB=1024*1024;
const now=()=>performance.now();
const waiters=new Set();
function signal(){for(const r of waiters)r();waiters.clear();}
async function changed(){await new Promise(r=>waiters.add(r));}
const agent=new http.Agent({keepAlive:true,maxSockets:8});

function seal(key,tid,index,plain){
  const nonce=Buffer.alloc(12);nonce.writeBigUInt64BE(BigInt(index),4);
  const cipher=crypto.createCipheriv('aes-256-gcm',key,nonce);
  cipher.setAAD(Buffer.from(`${tid}:0:0:${index}:${plain.length}`));
  const encrypted=Buffer.concat([cipher.update(plain),cipher.final(),cipher.getAuthTag()]);
  const header=Buffer.alloc(8);header.writeUInt32BE(index);header.writeUInt32BE(encrypted.length,4);
  return Buffer.concat([header,encrypted]);
}
function open(key,tid,index,encrypted){
  const len=encrypted.length-16,nonce=Buffer.alloc(12);nonce.writeBigUInt64BE(BigInt(index),4);
  const decipher=crypto.createDecipheriv('aes-256-gcm',key,nonce);
  decipher.setAAD(Buffer.from(`${tid}:0:0:${index}:${len}`));
  decipher.setAuthTag(encrypted.subarray(len));
  return Buffer.concat([decipher.update(encrypted.subarray(0,len)),decipher.final()]);
}
async function* packets(readable){
  let pending=Buffer.alloc(0);
  for await(const chunk of readable){
    pending=Buffer.concat([pending,chunk]);
    while(pending.length>=8){
      const len=pending.readUInt32BE(4);
      if(len<16||len>FRAME+16)throw Error('invalid_frame_size');
      if(pending.length<8+len)break;
      yield {index:pending.readUInt32BE(),encrypted:pending.subarray(8,8+len),wire:pending.subarray(0,8+len)};
      pending=pending.subarray(8+len);
    }
  }
  if(pending.length)throw Error('truncated_frame');
}
function request(port,method,url,token,body){
  return new Promise((resolve,reject)=>{
    const req=http.request({host:'127.0.0.1',port,method,path:url,agent,
      headers:{authorization:`Bearer ${token}`}},res=>{
      if(res.statusCode!==200){res.resume();reject(Error(`http_${res.statusCode}`));}
      else resolve(res);
    });
    req.on('error',reject);
    if(body)req.end(body);else req.end();
  });
}
async function drain(res){for await(const _ of res){};}

async function runCase(root,size,rate){
  const dir=await fsp.mkdtemp(path.join(root,'case-'));
  const src=path.join(dir,'synthetic.bin'),out=path.join(dir,'received.bin');
  const spool=path.join(dir,'spool');await fsp.mkdir(spool);
  const block=crypto.randomBytes(FRAME),fh=await fsp.open(src,'wx');
  for(let offset=0;offset<size;offset+=block.length)await fh.write(block,0,Math.min(block.length,size-offset));
  await fh.datasync();await fh.close();
  const sourceStats=await fsp.stat(src);
  const token=crypto.randomBytes(32).toString('hex'),key=crypto.randomBytes(32),tid=crypto.randomUUID();
  const cap=Math.min(Math.floor(size*.20),1_000_000_000);
  // Charge allocated data blocks reported by stat, not only ciphertext lengths.
  let used=0,peak=0,prepared=0,uploadDone=false,uploadTotal=0,ackTotal=0,stalls=0,stallMs=0;
  let firstServerDiskMs=null,firstForwardMs=null,firstAuthenticatedMs=null,firstDurableAckMs=null;
  const frames=new Map();
  let expectedDigest;
  const started=now();
  const server=http.createServer(async(req,res)=>{
    try{
      if(req.headers.authorization!==`Bearer ${token}`){res.writeHead(403);res.end();return;}
      if(req.method==='POST'&&req.url==='/upload'){
        let expected=0;
        for await(const frame of packets(req)){
          if(frame.index!==expected++)throw Error('frame_order');
          const cost=Math.ceil(frame.wire.length/4096)*4096;
          if(cost>cap)throw Error('frame_exceeds_window');
          if(used+cost>cap){const at=now();stalls++;while(used+cost>cap)await changed();stallMs+=now()-at;}
          used+=cost;peak=Math.max(peak,used);
          const p=path.join(spool,`frame-${frame.index}`),fd=await fsp.open(p,'wx');
          await fd.writeFile(frame.wire);await fd.datasync();await fd.close();
          const allocated=(await fsp.stat(p)).blocks*512;
          if(allocated>cost)throw Error('disk_allocation_exceeds_reservation');
          frames.set(frame.index,{p,cost});prepared++;uploadTotal+=frame.encrypted.length-16;
          if(firstServerDiskMs===null)firstServerDiskMs=now()-started;
          signal();
        }
        if(uploadTotal!==size)throw Error('upload_length');
        uploadDone=true;signal();res.end('uploaded');
      }else if(req.method==='GET'&&req.url==='/download'){
        res.writeHead(200,{'content-type':'application/octet-stream'});res.flushHeaders();
        let next=0;
        while(true){
          while(next>=prepared&&!uploadDone)await changed();
          if(next>=prepared&&uploadDone)break;
          const frame=frames.get(next);if(!frame)throw Error('missing_spool_frame');
          const wire=await fsp.readFile(frame.p);
          if(firstForwardMs===null)firstForwardMs=now()-started;
          if(!res.write(wire))await once(res,'drain');
          next++;
        }
        res.end();
      }else if(req.method==='POST'&&req.url?.startsWith('/ack/')){
        const index=Number(req.url.substring(5)),frame=frames.get(index);
        if(!frame)throw Error('invalid_ack');
        // Caller sends this only AFTER authenticated receive and fdatasync.
        await fsp.unlink(frame.p);frames.delete(index);used-=frame.cost;ackTotal++;signal();
        if(firstDurableAckMs===null)firstDurableAckMs=now()-started;
        res.end('ack');
      }else{res.writeHead(404);res.end();}
    }catch(error){res.destroy(error);signal();}
  });
  await new Promise(r=>server.listen(0,'127.0.0.1',r));
  const port=server.address().port;
  let uploadFinishedMs,receiveFinishedMs;
  try{
    const receiving=(async()=>{
      const response=await request(port,'GET','/download',token);
      const dest=await fsp.open(out,'wx'),digest=crypto.createHash('sha256');
      let bytes=0,next=0,rateStarted;
      for await(const frame of packets(response)){
        if(frame.index!==next++)throw Error('receive_frame_order');
        const plain=open(key,tid,frame.index,frame.encrypted);
        if(firstAuthenticatedMs===null){firstAuthenticatedMs=now()-started;rateStarted=now();}
        await dest.writeFile(plain);await dest.datasync();digest.update(plain);bytes+=plain.length;
        if(rate){const delay=bytes/rate*1000-(now()-rateStarted);if(delay>0)await pause(delay);}
        await drain(await request(port,'POST',`/ack/${frame.index}`,token));
      }
      await dest.close();receiveFinishedMs=now()-started;
      return {bytes,digest:digest.digest('hex')};
    })();
    const sending=(async()=>{
      const req=http.request({host:'127.0.0.1',port,method:'POST',path:'/upload',agent,
        headers:{authorization:`Bearer ${token}`,'x-declared-source-bytes':size}},res=>{res.resume();});
      const finished=new Promise((resolve,reject)=>{req.on('response',res=>{res.on('end',()=>res.statusCode===200?resolve():reject(Error('upload_failed')));});req.on('error',reject);});
      const digest=crypto.createHash('sha256');let index=0;
      for await(const plain of fs.createReadStream(src,{highWaterMark:FRAME})){
        digest.update(plain);if(!req.write(seal(key,tid,index++,plain)))await once(req,'drain');
      }
      expectedDigest=digest.digest('hex');req.end();await finished;uploadFinishedMs=now()-started;
    })();
    const [received]=await Promise.all([receiving,sending]);
    const after=await fsp.stat(src);
    const valid=received.bytes===size&&received.digest===expectedDigest&&sourceStats.size===after.size&&sourceStats.mtimeMs===after.mtimeMs;
    if(!valid||used!==0||frames.size!==0||peak>cap)throw Error('verification_failed');
    return {case:rate?'slow_receiver':'unthrottled',source_bytes:size,frame_plaintext_max_bytes:FRAME,
      disk_window_cap_bytes:cap,peak_disk_frame_blocks_bytes:peak,disk_window_respected:peak<=cap,
      first_server_disk_durable_ms:firstServerDiskMs,first_forward_ms:firstForwardMs,
      first_authenticated_file_bytes_ms:firstAuthenticatedMs,first_receiver_durable_ack_ms:firstDurableAckMs,
      upload_finished_ms:uploadFinishedMs,receive_finished_ms:receiveFinishedMs,
      disk_credit_stalls:stalls,disk_credit_wait_ms:stallMs,receiver_rate_limit_bytes_per_second:rate||null,
      frames_durable_acked:ackTotal,byte_count_and_sha256_match:valid,remaining_spool_bytes:used,
      scope:'DEV_loopback_HTTP_real_disk_AEAD_no_WAN_no_crash_recovery_no_product_UI'};
  }finally{
    server.closeAllConnections();await new Promise(r=>server.close(r));await fsp.rm(dir,{recursive:true,force:true});
  }
}
const root=await fsp.mkdtemp(path.join(os.tmpdir(),'dashbeam-disk-spool-probe-'));
try{
  for(const [size,rate] of [[MIB,0],[32*MIB,0],[32*MIB,4*MIB]]){
    console.log(JSON.stringify({observed_utc:new Date().toISOString(),result:await runCase(root,size,rate)}));
  }
}finally{agent.destroy();await fsp.rm(root,{recursive:true,force:true});}
