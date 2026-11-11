import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const root = path.resolve(process.argv[2]);
assert(root.startsWith('/home/luoji/code/dashbeam-benchmark-20261111.'));
const sha = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const manifests = {};
for (const variant of ['baseline', 'candidate']) {
  const instrumentation = JSON.parse(fs.readFileSync(path.join(root, 'results', `instrument-${variant}-v2.json`)));
  for (const file of instrumentation.files) assert.equal(sha(path.join(instrumentation.root, file.file)), file.after_sha256);
  assert.equal(sha(path.join(instrumentation.root, 'engine/examples/latency_harness.rs')), sha(path.join(root, 'tools/latency_harness.rs')));
  manifests[variant] = { revision: variant === 'baseline' ? 'f3253bbbf6c1028821e2a66602ede9557cff4113' : '2ef67abf2e100f32db9aca7834397e705250053e',
    binary_sha256: sha(path.join(root, 'bin', variant)), lock_sha256: sha(path.join(instrumentation.root, 'engine/Cargo.lock')), instrumentation };
}
assert.notEqual(manifests.baseline.binary_sha256, manifests.candidate.binary_sha256, 'aliased build cache');
fs.writeFileSync(path.join(root, 'results/build-v2.json'), JSON.stringify({
  recorded_utc: new Date().toISOString(), variants: manifests,
  rustc: fs.readFileSync(path.join(root, 'results/rustc.txt'), 'utf8').trim(),
  cargo: fs.readFileSync(path.join(root, 'results/cargo.txt'), 'utf8').trim(),
  profile: { opt_level: 2, lto: false, codegen_units: 8, build_jobs: 2, locked: true, offline: true },
  cache_note: 'each archived engine source touched before its build, then copied before the next build; all original before hashes checked against Git revisions by coordinator',
  fixture_sha256: sha(path.join(root, 'fixtures/manifest.json')),
  relay_binary_sha256: sha(path.join(root, 'bin/iroh-relay')),
}, null, 2) + '\n');
