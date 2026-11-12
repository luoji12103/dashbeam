// Bounded, synthetic DEV-only CLI acceptance; never uses saved app settings.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
const root=path.resolve(process.argv[2]||'');
assert(root.startsWith('/home/luoji/code/dashbeam-source-stream-20261112.'));
const bin=path.join(root,'bin/source_stream'),source=path.join(root,'fixtures/one-mib.bin');
const owned=fs.mkdtempSync(path.join(root,'cli-validation-'));
fs.chmodSync(owned,0o700);
const ticket=path.join(owned,'ticket'),output=path.join(owned,'output');fs.mkdirSync(output,{mode:0o700});
let server,serverExit,stdout='',stderr='';
const proof={binary_sha256:crypto.createHash('sha256').update(fs.readFileSync(bin)).digest('hex')};
try{
 const noNetwork=spawnSync(bin,['send',source,'--ticket-file',ticket],{encoding:'utf8',timeout:10000});
 assert.equal(noNetwork.status,1);assert(noNetwork.stderr.includes('choose explicit'));
 assert(!fs.existsSync(ticket));proof.no_implicit_public_network=true;
 const badWindow=spawnSync(bin,['--direct','--transport-window-mib','17','send',source,'--ticket-file',ticket],{encoding:'utf8',timeout:10000});
 assert.equal(badWindow.status,2);proof.invalid_window_rejected=true;
 server=spawn(bin,['--direct','send',source,'--ticket-file',ticket],{stdio:['ignore','pipe','pipe']});
 server.stdout.on('data',b=>{if(stdout.length<4096)stdout+=b;});
 server.stderr.on('data',b=>{if(stderr.length<4096)stderr+=b;});
 serverExit=new Promise((resolve,reject)=>{server.once('exit',(code,signal)=>resolve({code,signal}));server.once('error',reject);});
 const end=Date.now()+10000;
 while(!fs.existsSync(ticket)){assert(Date.now()<end,'CLI ticket timeout');assert.equal(server.exitCode,null);await new Promise(r=>setTimeout(r,20));}
 assert.equal(fs.statSync(ticket).mode&0o777,0o600);proof.ticket_mode='0600';
 const recv=spawnSync(bin,['--direct','receive','--ticket-file',ticket,'--output',output],{encoding:'utf8',timeout:15000});
 assert.equal(recv.status,0,recv.stderr);
 const files=fs.readdirSync(output);assert.deepEqual(files,['one-mib.bin']);
 const bytes=fs.readFileSync(path.join(output,files[0]));assert(bytes.equals(fs.readFileSync(source)));
 proof.direct_cli_bytes_match=true;proof.bytes=bytes.length;
 assert(recv.stdout.includes('Completion receipt transport-delivered: true'));proof.completion_receipt_transport_delivered=true;
 server.kill('SIGINT');
 const timer=setTimeout(()=>server.kill('SIGTERM'),8000);
 const ended=await serverExit;clearTimeout(timer);assert.equal(ended.code,0,stderr);proof.sender_ctrl_c_clean_exit=true;
 fs.writeFileSync(path.join(root,'results/cli-validation.json'),JSON.stringify(proof,null,2)+'\n',{flag:'wx'});
 console.log(JSON.stringify(proof));
}finally{
 if(server&&server.exitCode===null){server.kill('SIGTERM');await serverExit;}
 // Only the exact synthetic directory created above, never an app data folder.
 fs.rmSync(owned,{recursive:true});
}
