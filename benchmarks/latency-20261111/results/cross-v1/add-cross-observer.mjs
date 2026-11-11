// Apply only after instrument.mjs v2 to a separate benchmark source copy.
// Read the receiver endpoint's connected home-relay snapshot at payload GetDone.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const root = process.argv[2];
assert(root && path.isAbsolute(root));
const file = 'engine/protocol/src/receive.rs';
const target = path.join(root, file);
const before = fs.readFileSync(target, 'utf8');
assert(before.includes('benchmark-get-done'), 'v2 instrumentation required');
assert(!before.includes('benchmark-recv-home-a'), 'cross observer already applied');
const anchor = '                    if let Some(h) = app_handle.as_ref() { let _ = h.emit_event("benchmark-get-done"); }';
assert.equal(before.split(anchor).length - 1, 1, 'expected exactly one v2 GetDone marker');
const after = before.replace(anchor, `${anchor}
                    {
                        use iroh::Watcher as _;
                        for status in endpoint.home_relay_status().get() {
                            if status.is_connected() {
                                if let Some(h) = app_handle.as_ref() {
                                    match status.url().host_str() {
                                        Some("relay.dashbeam.test") => { let _ = h.emit_event("benchmark-recv-home-a"); }
                                        Some("relay-b.dashbeam.test") => { let _ = h.emit_event("benchmark-recv-home-b"); }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }`);
fs.writeFileSync(target, after);
const hash = s => crypto.createHash('sha256').update(s).digest('hex');
console.log(JSON.stringify({ root, instrumentation: 'cross-home-observer-v1',
  files: [{ file, before_sha256: hash(before), after_sha256: hash(after) }] }));
