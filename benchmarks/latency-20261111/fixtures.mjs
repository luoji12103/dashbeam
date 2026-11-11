import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const root = process.argv[2];
assert(root && path.isAbsolute(root));
fs.mkdirSync(root, { recursive: true });
function file(name, size, seed) {
  const target = path.join(root, name);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  const fd = fs.openSync(target, 'wx', 0o644);
  const buffer = Buffer.alloc(65536);
  const hash = crypto.createHash('sha256');
  let state = seed >>> 0;
  for (let left = size; left > 0;) {
    const n = Math.min(left, buffer.length);
    for (let i = 0; i < n; i++) {
      state ^= state << 13; state ^= state >>> 17; state ^= state << 5;
      buffer[i] = state & 255;
    }
    fs.writeSync(fd, buffer, 0, n);
    hash.update(buffer.subarray(0, n));
    left -= n;
  }
  fs.fsyncSync(fd); fs.closeSync(fd);
  return { name, size, seed, sha256: hash.digest('hex') };
}
const files = [file('one-mib.bin', 1048576, 1701),
  file('sixteen-mib.bin', 16 * 1048576, 1702),
  file('sixtyfour-mib.bin', 64 * 1048576, 1703)];
for (let i = 0; i < 256; i++) files.push(file(`many-small/${String(i).padStart(4, '0')}.bin`, 4096, 2000 + i));
fs.writeFileSync(path.join(root, 'manifest.json'), JSON.stringify({ files, cache_policy: 'pre-read same fixture before each timed sample; process/endpoint cold, file cache warm' }, null, 2));
console.log(JSON.stringify({ files: files.length, bytes: files.reduce((s, f) => s + f.size, 0) }));
