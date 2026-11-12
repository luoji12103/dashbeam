// Run on the isolated Debian DEV root AFTER the documented build succeeds.
// Captures the actual instrumented sources and immutable binary, not just HEAD.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
const root=path.resolve(process.argv[2]||'');
assert(root.startsWith('/home/luoji/code/dashbeam-source-stream-20261112.'));
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const command=(bin,args)=>execFileSync(bin,args,{encoding:'utf8'}).trim();
function files(dir){return fs.readdirSync(dir,{withFileTypes:true}).flatMap(e=>{
 const p=path.join(dir,e.name);if(e.isDirectory())return ['target','.git'].includes(e.name)?[]:files(p);
 return /\.(rs|toml|lock)$/.test(e.name)?[p]:[];
});}
const source=path.join(root,'source');
const sourceHashes=Object.fromEntries(files(path.join(source,'engine')).sort().map(p=>[path.relative(source,p),sha(p)]));
const proof={
 base_commit:'431e8376714d7ef44034b3b32c239c4bbef5fe90',
 description:'Archived compatible base plus experimental streaming sources and observation-only benchmark hooks. See source_sha256 for exact measured code.',
 recorded_at:new Date().toISOString(),
 host:command('hostname',[]),kernel:command('uname',['-r']),
 rustc:command('/home/luoji/.cargo/bin/rustc',['-Vv']),cargo:command('/home/luoji/.cargo/bin/cargo',['-V']),node:process.version,
 build:{manifest:'source/engine/Cargo.toml',args:['--locked','--offline','--release','--example','source_stream_bench','--example','source_stream'],
  env:{CARGO_BUILD_JOBS:'2',CARGO_PROFILE_RELEASE_OPT_LEVEL:'2',CARGO_PROFILE_RELEASE_LTO:'false',CARGO_PROFILE_RELEASE_CODEGEN_UNITS:'8',
   CARGO_TARGET_DIR:'/home/luoji/code/dashbeam-macos/src-tauri/target'},
  commands:['cargo test --manifest-path source/engine/Cargo.toml --locked --offline --release -p sendme-native stream::tests -- --test-threads=1',
   'cargo build --manifest-path source/engine/Cargo.toml --locked --offline --release --example source_stream_bench --example source_stream']},
 source_sha256:sourceHashes,
 instrumentation:JSON.parse(fs.readFileSync(path.join(root,'results/instrumentation.json'))),
 tools_sha256:Object.fromEntries(['instrument.mjs','run.mjs','source_stream_bench.rs','summarize.mjs','record-build.mjs'].map(f=>[f,sha(path.join(root,'tools',f))])),
 binary_sha256:Object.fromEntries(['source_stream_bench','source_stream','iroh-relay'].map(f=>[f,sha(path.join(root,'bin',f))])),
 checks_sha256:Object.fromEntries(['stream-tests-final.txt','build-final.txt'].map(f=>[f,sha(path.join(root,'results',f))])),
};
fs.writeFileSync(path.join(root,'results/build.json'),JSON.stringify(proof,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({host:proof.host,binary_sha256:proof.binary_sha256}));
