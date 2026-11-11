import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const root = path.resolve(process.argv[2]);
assert(root.startsWith('/home/luoji/code/dashbeam-benchmark-20261111.'));
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(path.join(root, file))).digest('hex');
assert.notEqual(sha('bin/candidate'), sha('bin/optimized'));
fs.writeFileSync(path.join(root, 'results/build-iteration.json'), JSON.stringify({
  base_revision: '2ef67abf2e100f32db9aca7834397e705250053e',
  base_binary_sha256: sha('bin/candidate'), optimized_binary_sha256: sha('bin/optimized'),
  patch_sha256: sha('tools/export-parent-cache.patch'),
  before_export_sha256: sha('candidate-v2/engine/native/src/export.rs'),
  after_export_sha256: sha('optimized-v2/engine/native/src/export.rs'),
  harness_sha256: sha('optimized-v2/engine/examples/latency_harness.rs'),
  test_log: 'export-cache-tests.txt', test_log_sha256: sha('results/export-cache-tests.txt'),
  status: 'isolated prototype only; not applied to product source or release',
}, null, 2) + '\n');
