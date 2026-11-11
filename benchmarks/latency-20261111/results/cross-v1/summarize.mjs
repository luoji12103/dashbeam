// Summarize every attempted transfer; never hide failures or label n=5 as p95.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const dir = path.resolve(process.argv[2]);
const rows = fs.readFileSync(path.join(dir, 'samples.jsonl'), 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse);
const median = a => {
  if (!a.length) return null;
  a = [...a].sort((x, y) => x - y);
  return (a[Math.floor((a.length - 1) / 2)] + a[Math.floor(a.length / 2)]) / 2;
};
const stats = a => ({ n: a.length, median: median(a), min: a.length ? Math.min(...a) : null, max: a.length ? Math.max(...a) : null });
const groups = [];
for (const id of [...new Set(rows.map(r => r.case.id))].sort((a, b) => a - b)) {
  const group = rows.filter(r => r.case.id === id);
  const variants = [...new Set(group.map(r => r.variant))];
  const baseline = variants.includes('baseline') ? 'baseline' : 'candidate';
  const metrics = [...new Set(group.flatMap(r => Object.keys(r.metrics || {})))];
  const summary = { case: group[0].case, comparison_baseline: baseline, variants: {}, paired: {} };
  for (const variant of variants) {
    const samples = group.filter(r => r.variant === variant);
    assert.equal(new Set(samples.map(r => r.pair)).size, samples.length, 'duplicate variant/pair');
    const valid = samples.filter(r => r.success && r.path_verified);
    summary.variants[variant] = { attempted: samples.length, successful: samples.filter(r => r.success).length,
      path_verified: valid.length, metrics: Object.fromEntries(metrics.map(m => [m, stats(valid.map(r => r.metrics[m]).filter(Number.isFinite))])) };
  }
  for (const variant of variants.filter(v => v !== baseline)) {
    const deltas = Object.fromEntries(metrics.map(m => [m, []]));
    for (const r of group.filter(r => r.variant === variant && r.success && r.path_verified)) {
      const old = group.find(b => b.variant === baseline && b.pair === r.pair && b.success && b.path_verified);
      if (!old) continue;
      for (const m of metrics) if (Number.isFinite(old.metrics[m]) && Number.isFinite(r.metrics[m])) deltas[m].push(old.metrics[m] - r.metrics[m]);
    }
    summary.paired[variant] = Object.fromEntries(metrics.map(m => [m, stats(deltas[m])]));
  }
  groups.push(summary);
}
const result = { attempts: rows.length, successes: rows.filter(r => r.success).length,
  path_verified: rows.filter(r => r.path_verified).length, groups };
fs.writeFileSync(path.join(dir, 'summary.json'), JSON.stringify(result, null, 2) + '\n');
const sec = n => Number.isFinite(n) ? (n / 1000).toFixed(3) : '—';
const lines = [
  '# DashBeam latency benchmark: measured summary', '',
  `${result.successes}/${result.attempts} transfers passed byte verification; ${result.path_verified}/${result.attempts} also passed the expected-path check.`, '',
  'Times below are medians in seconds; `payload` means the first internal blobs payload-progress proxy, not first wire byte. `return` includes export, advisory wait and endpoint cleanup, but excludes the separately measured byte-verification pass. All failed attempts remain in the raw denominator.', '',
  '| Case | Version | Verified / attempted | Share ready | Receive → payload | Share → payload | Share → return |',
  '| --- | --- | --- | ---: | ---: | ---: | ---: |',
];
for (const g of groups) for (const [v, s] of Object.entries(g.variants)) {
  const m = name => sec(s.metrics[name]?.median);
  lines.push(`| ${g.case.id}. ${g.case.name} | ${v} | ${s.path_verified}/${s.attempted} | ${m('share_ready_ms')} | ${m('receive_payload_progress_ms')} | ${m('end_to_end_payload_progress_ms')} | ${m('end_to_end_valid_export_ms')} |`);
}
lines.push('', '## Paired savings', '',
  'Positive numbers mean faster than the matched baseline. These are medians of per-pair differences, not a subtraction of the two column medians. Small samples support median/range only; no p95 claims.', '',
  '| Case | Version | Pairs | Share → payload saved | Difference range | Share → return saved |',
  '| --- | --- | ---: | ---: | --- | ---: |');
for (const g of groups) for (const [v, p] of Object.entries(g.paired)) {
  const m = p.end_to_end_payload_progress_ms;
  lines.push(`| ${g.case.id} | ${v} | ${m?.n || 0} | ${sec(m?.median)} | ${sec(m?.min)}–${sec(m?.max)} | ${sec(p.end_to_end_valid_export_ms?.median)} |`);
}
lines.push('', '## Scope', '',
  '- Frozen source revisions, identical instrumentation, fixed compiler/profile and CPU/memory limits; see environment/build manifests.',
  '- Real Rust engine + simulated application command orchestration. This is not a full GUI/Android SAF/phone-to-Mac test.',
  '- Each sample uses a fresh process, endpoint, ephemeral identity, store and output. Source page cache is equally warmed.',
  '- Direct/relay paths are checked on the real connection.',
  ...(groups.some(g => g.case.id === 10) ? ['- Case 10 injects advisory-only sleep and is a synthetic timeout stress case.'] : []),
  groups.some(g => g.case.id === 11)
    ? '- One isolated Linux kernel and Docker bridge; case 11 uses two independent local relays, with sender configured only A and receiver only B. Home snapshots and selected relay path are verified, not a continuously traced A→B payload route. Both-URL automatic home selection is not tested.'
    : '- One isolated Linux kernel, Docker bridge and local relay.',
  '- Internet routing, GZ↔SH geographic links, NAT varieties, mobile sleep and production authentication are not represented.',
  '- See per-case ping calibration and per-sample qdisc counters. Timing includes a shared-file ticket handoff with a 20 ms polling interval.',
  '- Output API completion is not a power-loss durability/fsync guarantee.', '');
fs.writeFileSync(path.join(dir, 'RESULTS.md'), lines.join('\n'));
console.log(JSON.stringify({ attempts: result.attempts, successes: result.successes, path_verified: result.path_verified, cases: groups.length }));
