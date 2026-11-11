import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const script = fileURLToPath(new URL('./summarize.mjs', import.meta.url));
const row = (variant, pair, success, pathVerified, value) => ({
  case: { id: 1, name: 'synthetic-summary-test' }, variant, pair, success, path_verified: pathVerified,
  metrics: Number.isFinite(value) ? { end_to_end_payload_progress_ms: value, end_to_end_valid_export_ms: value + 10 } : {},
});
function run(rows, check) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dashbeam-summary-test-'));
  try {
    fs.writeFileSync(path.join(dir, 'samples.jsonl'), rows.map(JSON.stringify).join('\n') + '\n');
    const execute = () => execFileSync(process.execPath, [script, dir], { stdio: 'pipe' });
    check(execute, () => JSON.parse(fs.readFileSync(path.join(dir, 'summary.json'))));
  } finally { fs.rmSync(dir, { recursive: true }); }
}
test('failures and path mismatches stay in denominators; savings use valid pairs only', () => {
  run([row('baseline', 1, true, true, 100), row('candidate', 1, true, true, 80),
    row('baseline', 2, false, false), row('candidate', 2, true, true, 95),
    row('baseline', 3, true, true, 110), row('candidate', 3, true, false, 92)], (execute, read) => {
    execute(); const s = read();
    assert.equal(s.attempts, 6); assert.equal(s.successes, 5); assert.equal(s.path_verified, 4);
    assert.equal(s.groups[0].variants.baseline.attempted, 3);
    assert.equal(s.groups[0].paired.candidate.end_to_end_payload_progress_ms.n, 1);
    assert.equal(s.groups[0].paired.candidate.end_to_end_payload_progress_ms.median, 20);
  });
});
test('all-failure groups produce empty stats rather than a fabricated time', () => {
  run([row('baseline', 1, false, false), row('candidate', 1, false, false)], (execute, read) => {
    execute(); assert.equal(read().successes, 0); assert.equal(read().groups[0].variants.baseline.path_verified, 0);
  });
});
test('duplicate pair identifiers are rejected', () => {
  run([row('baseline', 1, true, true, 100), row('baseline', 1, true, true, 110)], execute => assert.throws(execute));
});
