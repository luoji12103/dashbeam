// Identical, observation-only instrumentation of both archived revisions.
// Does not change timeout, retry, routing, payload or scheduling policies.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const root = process.argv[2];
assert(root && path.isAbsolute(root));
const manifest = [];
const hash = s => crypto.createHash('sha256').update(s).digest('hex');
function edit(file, fn) {
  const target = path.join(root, file);
  const before = fs.readFileSync(target, 'utf8');
  assert(!before.includes('benchmark-'), `${file} already instrumented`);
  const after = fn(before);
  fs.writeFileSync(target, after);
  manifest.push({ file, before_sha256: hash(before), after_sha256: hash(after) });
}
function once(s, from, to) {
  assert.equal(s.split(from).length - 1, 1, `expected one anchor: ${from}`);
  return s.replace(from, to);
}
const emit = (handle, name) => `if let Some(h) = ${handle}.as_ref() { let _ = h.emit_event("${name}"); }`;
// Metadata fetch has no AppHandle. This benchmark-only callback is installed
// once per process by the harness and adds observation without changing APIs.
edit('engine/protocol/src/types.rs', s => s + `
#[cfg(not(target_arch = "wasm32"))]
static BENCHMARK_OBSERVER: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();
#[cfg(not(target_arch = "wasm32"))]
pub fn benchmark_set_observer(observer: AppHandle) { let _ = BENCHMARK_OBSERVER.set(observer); }
#[cfg(not(target_arch = "wasm32"))]
pub fn benchmark_emit(name: &str) {
    if let Some(Some(observer)) = BENCHMARK_OBSERVER.get() { let _ = observer.emit_event(name); }
}
`);
edit('engine/src/lib.rs', s => s + `
#[cfg(not(target_arch = "wasm32"))]
pub use protocol::benchmark_set_observer;
`);
edit('engine/native/src/send.rs', s => {
  s = once(s, 'let endpoint = builder.bind().await?;',
    `let endpoint = builder.bind().await?;\n        ${emit('app_handle_clone', 'benchmark-send-endpoint-bound')}`);
  if (s.includes('let outcome = run_share_session(')) {
    s = once(s, '        anyhow::Ok(())\n    };',
      '        protocol::benchmark_emit("benchmark-send-online-done");\n        anyhow::Ok(())\n    };');
    s = once(s, '    let prepared = tokio::try_join!(online, crate::import::import_paths(paths, store.as_ref()));',
      `    let imported = async {
        let result = crate::import::import_paths(paths, store.as_ref()).await;
        if result.is_ok() { protocol::benchmark_emit("benchmark-send-import-done"); }
        result
    };
    let prepared = tokio::try_join!(online, imported);`);
  } else {
    s = once(s, '            crate::import::import_paths(canonical_paths, blobs.store()).await?;',
      '            crate::import::import_paths(canonical_paths, blobs.store()).await?;\n        protocol::benchmark_emit("benchmark-send-import-done");');
  }
  return s;
});
edit('engine/protocol/src/send.rs', s => {
  // The new version completes its online wait in prepare_share_content; the
  // old one does so in run_share_session after the import.
  if (!s.includes('if let Err(error) = timeout(Duration::from_secs(30), endpoint.online()).await')) {
    s = once(s, '    .await?;\n\n    let hash = temp_tag.hash();\n\n    let mut addr = router.endpoint().addr();',
      '    .await?;\n    crate::benchmark_emit("benchmark-send-online-done");\n\n    let hash = temp_tag.hash();\n\n    let mut addr = router.endpoint().addr();');
  }
  return s;
});
edit('engine/native/src/receive.rs', s => {
  const bind = s.includes('let endpoint = receive_endpoint_builder(&options)?.bind().await?;')
    ? 'let endpoint = receive_endpoint_builder(&options)?.bind().await?;'
    : 'let endpoint = builder.bind().await?;';
  s = once(s, bind, `${bind}\n    ${emit('app_handle', 'benchmark-recv-endpoint-bound')}`);
  const close = '            endpoint.close().await;';
  assert.equal(s.split(close).length - 1, 3, 'expected three receive close paths');
  return s.replaceAll(close, `            ${emit('app_handle', 'benchmark-recv-close-begin')}
            endpoint.close().await;
            ${emit('app_handle', 'benchmark-recv-close-end')}`);
});
edit('engine/protocol/src/receive.rs', s => {
  const pathEvent = prefix => `if let Some(h) = app_handle.as_ref() {
      let relay = benchmark_connection.paths().iter().any(|p| p.is_selected() && matches!(p.remote_addr(), iroh::TransportAddr::Relay(_)));
      let direct = benchmark_connection.paths().iter().any(|p| p.is_selected() && matches!(p.remote_addr(), iroh::TransportAddr::Ip(_)));
      let _ = h.emit_event(if relay { "${prefix}-relay" } else if direct { "${prefix}-ip" } else { "${prefix}-unknown" });
    }`;
  s = once(s, '        let sizes_result =',
    `        ${emit('app_handle', 'benchmark-connect-done')}\n        let benchmark_connection = connection.clone();\n        ${pathEvent('benchmark-initial-path')}\n        let sizes_result =`);
  s = once(s, '        emit_progress_event(app_handle, 0, payload_size, 0.0);',
    `        ${emit('app_handle', 'benchmark-sizes-done')}\n        emit_progress_event(app_handle, 0, payload_size, 0.0);`);
  s = once(s, '        let get = db.remote().execute_get(connection, local.missing());',
    `        ${emit('app_handle', 'benchmark-get-start')}\n        let mut benchmark_first = false;\n        let get = db.remote().execute_get(connection, local.missing());`);
  s = once(s, '                    speed.record(elapsed, received);',
    `                    if received > 0 && !benchmark_first {\n                        benchmark_first = true;\n                        ${emit('app_handle', 'benchmark-first-payload-progress')}\n                    }\n                    speed.record(elapsed, received);`);
  s = once(s, '                GetProgressItem::Done(value) => {',
    `                GetProgressItem::Done(value) => {\n                    if !benchmark_first && payload_size > 0 { ${emit('app_handle', 'benchmark-first-payload-progress')} }\n                    ${emit('app_handle', 'benchmark-get-done')}\n                    ${pathEvent('benchmark-final-path')}`);
  const fetchStart = s.indexOf('pub async fn fetch_metadata(');
  assert(fetchStart >= 0, 'metadata function missing');
  const prefix = s.slice(0, fetchStart);
  let metadata = s.slice(fetchStart);
  metadata = once(metadata, '                endpoint.close().await;',
    '                crate::benchmark_emit("benchmark-metadata-close-begin");\n                endpoint.close().await;\n                crate::benchmark_emit("benchmark-metadata-close-end");');
  metadata = once(metadata, '\n    endpoint.close().await;',
    '\n    crate::benchmark_emit("benchmark-metadata-close-begin");\n    endpoint.close().await;\n    crate::benchmark_emit("benchmark-metadata-close-end");');
  s = prefix + metadata;
  return s;
});
console.log(JSON.stringify({ root, instrumentation: 'observation-only-v2', files: manifest }));
