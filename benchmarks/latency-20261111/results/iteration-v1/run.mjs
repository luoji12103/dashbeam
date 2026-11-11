// DEV-only orchestration. Never applies tc/iptables to the host namespace.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
const root = path.resolve(process.argv[2] || '');
assert(root.startsWith('/home/luoji/code/dashbeam-benchmark-20261111.'));
const label = process.argv[3] || 'main';
assert(/^[a-z0-9-]+$/.test(label));
const repeats = Number(process.argv[4] || 5);
assert(repeats >= 1 && repeats <= 10);
const only = new Set((process.argv[5] || '').split(',').filter(Boolean).map(Number));
const variants = (process.argv[6] || 'baseline,candidate').split(',');
assert(variants.length === 2 && variants.every(v => ['baseline', 'candidate', 'optimized'].includes(v)) && new Set(variants).size === 2);
const experiment = `dbbench-1111-${label}-${crypto.randomBytes(3).toString('hex')}`;
const names = { send: `${experiment}-s`, recv: `${experiment}-r`, relay: `${experiment}-x` };
const results = path.join(root, 'results', label);
fs.mkdirSync(results, { recursive: false });
const output = path.join(results, 'samples.jsonl');
const image = 'dashbeam-benchmark:20261111';
const net = experiment;
const created = [];
let networkCreated = false;
const cases = [
  { id: 1, name: 'direct-small-lan', file: 'one-mib.bin', relay: false, rtt: 2, rate: 1000, loss: 0 },
  { id: 2, name: 'direct-large-lan', file: 'sixtyfour-mib.bin', relay: false, rtt: 2, rate: 1000, loss: 0 },
  { id: 3, name: 'direct-many-small', file: 'many-small', relay: false, rtt: 2, rate: 1000, loss: 0 },
  { id: 4, name: 'direct-small-wan', file: 'one-mib.bin', relay: false, rtt: 80, rate: 50, loss: 0 },
  { id: 5, name: 'direct-large-wan', file: 'sixtyfour-mib.bin', relay: false, rtt: 80, rate: 50, loss: 0 },
  { id: 6, name: 'relay-small-lan', file: 'one-mib.bin', relay: true, rtt: 2, rate: 1000, loss: 0 },
  { id: 7, name: 'relay-small-wan', file: 'one-mib.bin', relay: true, rtt: 80, rate: 50, loss: 0 },
  { id: 8, name: 'relay-large-wan', file: 'sixtyfour-mib.bin', relay: true, rtt: 80, rate: 50, loss: 0 },
  { id: 9, name: 'relay-lossy-slow', file: 'sixteen-mib.bin', relay: true, rtt: 150, rate: 20, loss: 0.5 },
  { id: 10, name: 'relay-metadata-stall', file: 'one-mib.bin', relay: true, rtt: 80, rate: 50, loss: 0, stall: 3500 },
].filter(c => !only.size || only.has(c.id));
function docker(...args) { return execFileSync('docker', args, { encoding: 'utf8', maxBuffer: 4 * 1024 * 1024 }).trim(); }
function exec(role, ...args) { return docker('exec', names[role], ...args); }
function launch(args) {
  const child = spawn('docker', args, { stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '', stderr = '';
  child.stdout.on('data', b => { if (stdout.length < 8192) stdout += b.toString(); });
  child.stderr.on('data', b => { if (stderr.length < 8192) stderr += b.toString(); });
  return new Promise(resolve => {
    child.on('error', e => resolve({ code: -1, error: e.message }));
    child.on('close', code => resolve({ code, stdout, stderr }));
  });
}
const sleep = ms => new Promise(r => setTimeout(r, ms));
function info(name) { return JSON.parse(docker('inspect', name))[0]; }
function ip(role) { return info(names[role]).NetworkSettings.Networks[net].IPAddress; }
function warm(target) {
  const st = fs.statSync(target);
  if (st.isDirectory()) { for (const f of fs.readdirSync(target).sort()) warm(path.join(target, f)); return; }
  const fd = fs.openSync(target, 'r');
  const b = Buffer.alloc(1048576);
  while (fs.readSync(fd, b, 0, b.length, null) > 0) { /* equal warm-cache policy */ }
  fs.closeSync(fd);
}
function shape(c, seed) {
  for (const [role, peer] of [['send', 'recv'], ['recv', 'send']]) {
    exec(role, 'iptables', '-F', 'OUTPUT');
    if (c.relay) exec(role, 'iptables', '-A', 'OUTPUT', '-p', 'udp', '-d', ip(peer), '-j', 'DROP');
    // Recreate rather than replace: reset counters and the seeded loss generator.
    if (JSON.parse(exec(role, 'tc', '-j', 'qdisc', 'show', 'dev', 'eth0')).some(q => q.kind === 'netem')) {
      exec(role, 'tc', 'qdisc', 'del', 'dev', 'eth0', 'root');
    }
    const args = ['tc', 'qdisc', 'add', 'dev', 'eth0', 'root', 'netem', 'limit', '10000',
      'delay', `${c.rtt / 2}ms`, 'rate', `${c.rate}mbit`];
    if (c.loss) args.push('loss', 'random', `${c.loss}%`, 'seed', String(seed + (role === 'recv' ? 1 : 0)));
    exec(role, ...args);
  }
}
function qdisc() {
  return Object.fromEntries(['send', 'recv'].map(r => [r, JSON.parse(exec(r, 'tc', '-s', '-j', 'qdisc', 'show', 'dev', 'eth0'))]));
}
function readJSON(file) { try { return JSON.parse(fs.readFileSync(file, 'utf8')); } catch { return null; } }
function duration(to, from) { return Number.isFinite(to) && Number.isFinite(from) ? (to - from) / 1e6 : null; }
function compute(s, r) {
  if (!s || !r || !r.bytes_match) return {};
  return {
    metadata_build_ms: duration(s.metadata_prepared_ns, s.share_intent_ns),
    send_relay_resolve_ms: duration(s.relay_resolved_ns, s.metadata_prepared_ns),
    engine_prepare_ms: duration(s.ticket_ready_ns, s.relay_resolved_ns),
    share_ready_ms: duration(s.ticket_ready_ns, s.share_intent_ns),
    receive_relay_resolve_ms: duration(r.relay_resolved_ns, r.receive_intent_ns),
    metadata_ms: duration(r.metadata_end_ns, r.metadata_start_ns),
    receive_payload_progress_ms: duration(r.first_payload_progress_proxy_ns, r.receive_intent_ns),
    end_to_end_payload_progress_ms: duration(r.first_payload_progress_proxy_ns, s.share_intent_ns),
    end_to_end_valid_export_ms: duration(r.receive_return_ns, s.share_intent_ns),
    receive_valid_export_ms: duration(r.receive_return_ns, r.receive_intent_ns),
    verification_ms: duration(r.verification_return_ns, r.receive_return_ns),
    connect_and_store_ms: duration(r.stages?.['benchmark-connect-done'], r.stages?.['benchmark-recv-endpoint-bound']),
    sizes_ms: duration(r.stages?.['benchmark-sizes-done'], r.stages?.['benchmark-connect-done']),
    payload_get_ms: duration(r.stages?.['benchmark-get-done'], r.stages?.['benchmark-get-start']),
    post_get_export_ms: duration(r.engine_completion_event_ns, r.stages?.['benchmark-get-done']),
    receive_close_ms: duration(r.stages?.['benchmark-recv-close-end'], r.stages?.['benchmark-recv-close-begin']),
    metadata_close_ms: duration(r.stages?.['benchmark-metadata-close-end'], r.stages?.['benchmark-metadata-close-begin']),
    send_bind_to_import_ms: duration(s.stages?.['benchmark-send-import-done'], s.stages?.['benchmark-send-endpoint-bound']),
    send_bind_to_online_ms: duration(s.stages?.['benchmark-send-online-done'], s.stages?.['benchmark-send-endpoint-bound']),
  };
}
async function sample(c, pair, variant) {
  shape(c, 4100 + pair * 2);
  const beforeQdisc = qdisc();
  const id = `${label}-c${c.id}-p${pair}-${variant}`;
  const relative = path.join('samples', id);
  const host = path.join(root, relative);
  fs.mkdirSync(host, { mode: 0o700 });
  fs.mkdirSync(path.join(host, 'tmp-send'), { mode: 0o700 });
  fs.mkdirSync(path.join(host, 'tmp-recv'), { mode: 0o700 });
  warm(path.join(root, 'fixtures', c.file));
  const containerShared = `/bench/${relative}`;
  const common = ['--variant', variant === 'baseline' ? 'baseline' : 'candidate', '--shared-dir', containerShared,
    '--source-path', `/fixtures/${c.file}`, '--relay', c.relay ? 'https://relay.dashbeam.test:3341' : 'disabled',
    '--metadata-stall-ms', String(c.stall || 0)];
  const run = role => launch(['exec', '--user', '1000:1000', '-e', `TMPDIR=${containerShared}/tmp-${role}`,
    '-e', 'RUST_LOG=off', '-e', 'SSL_CERT_FILE=/bench/tools/tls/ca.pem', names[role], 'timeout', '--signal=TERM', '--kill-after=5', '180',
    `/bench/bin/${variant}`, '--role', role, ...common]);
  const recv = run('recv');
  const send = run('send');
  const [sendExit, recvExit] = await Promise.all([send, recv]);
  const sender = readJSON(path.join(host, 'sender.json'));
  const receiver = readJSON(path.join(host, 'receiver.json'));
  const success = sendExit.code === 0 && recvExit.code === 0 && receiver?.bytes_match === true;
  const initialPath = Object.keys(receiver?.stages || {}).find(k => k.startsWith('benchmark-initial-path-'))?.split('-').at(-1);
  const finalPath = Object.keys(receiver?.stages || {}).find(k => k.startsWith('benchmark-final-path-'))?.split('-').at(-1);
  const pathVerified = success && initialPath === (c.relay ? 'relay' : 'ip') && finalPath === (c.relay ? 'relay' : 'ip');
  const row = { case: c, pair, variant, success, path_verified: pathVerified, initial_path: initialPath, final_path: finalPath,
    sender, receiver, metrics: compute(sender, receiver), process_exit: { send: sendExit, recv: recvExit }, qdisc_before: beforeQdisc, qdisc: qdisc() };
  fs.appendFileSync(output, JSON.stringify(row) + '\n', { mode: 0o600 });
  console.log(JSON.stringify({ case: c.id, pair, variant, success, path_verified: pathVerified, ...row.metrics }));
  // Only task-created synthetic exports; never touch user data or shared caches.
  const exported = path.join(host, 'output');
  assert(exported.startsWith(`${root}/samples/${label}-c`));
  if (fs.existsSync(exported)) fs.rmSync(exported, { recursive: true });
  for (const name of ['ticket', 'ticket.tmp']) {
    const file = path.join(host, name); if (fs.existsSync(file)) fs.unlinkSync(file);
  }
}
async function cleanup() {
  for (const name of created.reverse()) {
    try {
      assert.equal(info(name).Config.Labels['org.dashbeam.experiment'], experiment);
      docker('rm', '-f', name);
    } catch (e) { console.error(`cleanup ${name}: ${e.message}`); }
  }
  if (networkCreated) { try { docker('network', 'rm', net); } catch (e) { console.error(e.message); } }
}
try {
  docker('network', 'create', '--internal', '--label', `org.dashbeam.experiment=${experiment}`, net);
  networkCreated = true;
  for (const role of ['send', 'recv', 'relay']) {
    const args = ['run', '-d', '--name', names[role], '--network', net, '--cpus', role === 'relay' ? '1' : '2',
      '--memory', '768m', '--read-only', '--tmpfs', '/tmp:rw,nosuid,size=128m', '--cap-drop', 'ALL',
      '--label', `org.dashbeam.experiment=${experiment}`, '-v', `${root}:/bench:rw`, '-v', `${root}/fixtures:/fixtures:ro`];
    if (role !== 'relay') args.push('--cap-add', 'NET_ADMIN', '--cap-add', 'NET_RAW');
    else args.push('--network-alias', 'relay.dashbeam.test');
    args.push(image);
    docker(...args); created.push(names[role]);
  }
  docker('exec', '-d', '--user', '1000:1000', names.relay, '/bench/bin/iroh-relay', '--config-path', '/bench/tools/relay.toml');
  await sleep(500);
  assert(exec('relay', 'ss', '-lnt').includes(':3341'), 'relay HTTPS listener unavailable');
  assert(exec('relay', 'ss', '-lnu').includes(':7842'), 'relay QUIC address-discovery listener unavailable');
  const metadata = { experiment, cases, repeats, kernel: exec('send', 'uname', '-r'), image: info(names.send).Image,
    build: readJSON(path.join(root, 'results', 'build-v2.json')),
    iteration_build: variants.includes('optimized') ? readJSON(path.join(root, 'results', 'build-iteration.json')) : null,
    scripts: Object.fromEntries(['run.mjs', 'instrument.mjs', 'latency_harness.rs'].map(f => [f, crypto.createHash('sha256').update(fs.readFileSync(path.join(root, 'tools', f))).digest('hex')])),
    limits: { endpoint_cpus: 2, relay_cpus: 1, each_memory_bytes: 768 * 1048576, workers_per_endpoint: 2 },
    network: JSON.parse(docker('network', 'inspect', net)).map(n => ({ driver: n.Driver, internal: n.Internal, ipv6: n.EnableIPv6, ipam: n.IPAM })),
    link: JSON.parse(exec('send', 'ip', '-j', 'link', 'show', 'dev', 'eth0')),
    binaries: Object.fromEntries(variants.map(v => [v, crypto.createHash('sha256').update(fs.readFileSync(path.join(root, 'bin', v))).digest('hex')])),
    scope: 'real engine + simulated app orchestration; same-kernel monotonic clock; no GUI/SAF; internal HTTPS relay with trusted temporary CA and QUIC address discovery; warm source cache; cold per-transfer endpoints',
    fixture: 'v2: no TCP proxy; real TLS and QAD; qdisc recreated per sample; v1 smoke/partial-main discarded',
    relay: { url: 'https://relay.dashbeam.test:3341', quic_port: 7842, ca: 'ephemeral benchmark-only CA; not installed on host', authentication: 'none' },
    qdisc_placement: 'sender and receiver eth0 egress each RTT/2; isolated bridge/relay; no host qdisc', containers: names };
  fs.writeFileSync(path.join(results, 'environment.json'), JSON.stringify(metadata, null, 2));
  for (const c of cases) {
    shape(c, 4000);
    const ping = exec('send', 'ping', '-n', '-c', '3', '-i', '0.2', ip('recv'));
    fs.writeFileSync(path.join(results, `case-${c.id}-calibration.json`), JSON.stringify({ case: c, ping, qdisc: qdisc(),
      rules: Object.fromEntries(['send', 'recv'].map(r => [r, exec(r, 'iptables', '-S', 'OUTPUT')])) }, null, 2));
    for (let pair = 1; pair <= repeats; pair++) {
      const order = (pair + c.id) % 2 ? [...variants].reverse() : [...variants];
      for (const variant of order) await sample(c, pair, variant);
    }
  }
} finally { await cleanup(); }
