import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const root = path.resolve(process.argv[2]);
assert(root.startsWith('/home/luoji/code/dashbeam-benchmark-20261111.'));
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(path.join(root, file))).digest('hex');
const base = JSON.parse(fs.readFileSync(path.join(root, 'results/build-v2.json')));
const variants = {};
for (const variant of ['baseline', 'candidate']) {
  const dir = `${variant}-cross-v3`;
  const extra = JSON.parse(fs.readFileSync(path.join(root, 'results', `instrument-${variant}-cross.json`)));
  for (const file of base.variants[variant].instrumentation.files) {
    const observed = extra.files.find(x => x.file === file.file);
    if (observed) assert.equal(observed.before_sha256, file.after_sha256);
    assert.equal(sha(`${dir}/${file.file}`), observed?.after_sha256 || file.after_sha256);
  }
  assert.equal(sha(`${dir}/engine/examples/latency_harness.rs`), sha('tools/latency_harness.rs'));
  variants[variant] = { revision: base.variants[variant].revision, binary_sha256: sha(`bin/${variant}-cross`),
    lock_sha256: sha(`${dir}/engine/Cargo.lock`), additional_observation: extra };
}
assert.notEqual(variants.baseline.binary_sha256, variants.candidate.binary_sha256);
fs.writeFileSync(path.join(root, 'results/build-cross.json'), JSON.stringify({
  recorded_utc: new Date().toISOString(), variants, base_build: 'build-v2.json',
  profile: base.profile, rustc: base.rustc, cargo: base.cargo,
  harness_sha256: sha('tools/latency_harness.rs'), ca_sha256: sha('tools/tls/ca.pem'), relay_cert_sha256: sha('tools/tls/relay.pem'),
  topology: 'sender configured only A; receiver configured only B; no relay-to-relay mesh; peer UDP blocked; snapshots require actual home A != B',
  limitation: 'does not model both URLs on each device or automatic home selection; snapshots are not continuous tracing',
}, null, 2) + '\n');
