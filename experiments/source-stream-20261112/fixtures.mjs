import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const root=path.resolve(process.argv[2]||'');assert(root.startsWith('/home/luoji/code/dashbeam-source-stream-20261112.'));
const reused='/home/luoji/code/dashbeam-benchmark-20261111.6mQj0M';
fs.mkdirSync(path.join(root,'fixtures'),{recursive:false});
for(const [name,mb] of [['one-mib.bin',1],['sixteen-mib.bin',16],['sixtyfour-mib.bin',64]]){
 const source=path.join(reused,'fixtures',name);assert.equal(fs.statSync(source).size,mb*1048576);
 fs.linkSync(source,path.join(root,'fixtures',name));
}
const fd=fs.openSync(path.join(root,'fixtures','quarter-gib.bin'),'wx',0o600);
const block=crypto.randomBytes(1048576);try{for(let n=0;n<256;n++)fs.writeSync(fd,block);fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
console.log('Reused three prior synthetic inputs by hard link; generated one 256MiB fixture.');
