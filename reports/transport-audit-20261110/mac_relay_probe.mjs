// Read only the already configured relay credentials; never print them.
// The raw TLS/WebSocket probe uses fresh ephemeral keys and eight small pings.
// This is a Mac-to-relay baseline, not an application or file-transfer benchmark.
import fs from 'node:fs'
import tls from 'node:tls'
import crypto from 'node:crypto'

const settings = JSON.parse(JSON.parse(fs.readFileSync(
  '/Users/luoji12103/Library/Application Support/com.dashbeam.desktop/settings.json', 'utf8'
)).app_settings).state

function frame(payload, opcode = 2) {
  const mask = crypto.randomBytes(4)
  const data = Buffer.from(payload)
  for (let n = 0; n < data.length; n++) data[n] ^= mask[n % 4]
  return Buffer.concat([Buffer.from([0x80 | opcode, 0x80 | payload.length]), mask, data])
}

function probe(base) {
  return new Promise(resolve => {
    const u = new URL(base)
    const start = performance.now()
    const keys = crypto.generateKeyPairSync('ed25519')
    const pub = keys.publicKey.export({type: 'spki', format: 'der'}).subarray(-32)
    let buffer = Buffer.alloc(0), upgraded = false, ended = false, pingAt, nonce
    const result = {host: u.hostname, pingPongRttMs: []}
    const socket = tls.connect({host: u.hostname, port: u.port || 443, servername: u.hostname,
      ALPNProtocols: ['http/1.1'], rejectUnauthorized: true})
    function finish(extra = {}) {
      if (ended) return
      ended = true
      clearTimeout(timer)
      if (upgraded && socket.writable) socket.end(frame(Buffer.from([3, 232]), 8))
      else socket.destroy()
      setTimeout(() => socket.destroy(), 500).unref()
      resolve({...result, ...extra, totalMs: performance.now() - start})
    }
    const timer = setTimeout(() => finish({errorCategory: 'probe_timeout'}), 20000)
    function ping() {
      nonce = crypto.randomBytes(8)
      pingAt = performance.now()
      socket.write(frame(Buffer.concat([Buffer.from([9]), nonce])))
    }
    socket.on('secureConnect', () => {
      result.tlsAuthorized = socket.authorized
      result.tlsReadyMs = performance.now() - start
      const material = socket.exportKeyingMaterial(32, 'iroh-relay handshake v1', pub)
      const signature = crypto.sign(null, material.subarray(0, 16), keys.privateKey)
      const auth = Buffer.concat([pub, Buffer.from([64]), signature, material.subarray(16)]).toString('base64url')
      socket.write(['GET /relay HTTP/1.1', `Host: ${u.host}`, 'Connection: Upgrade',
        'Upgrade: websocket', 'Sec-WebSocket-Version: 13',
        `Sec-WebSocket-Key: ${crypto.randomBytes(16).toString('base64')}`,
        'Sec-WebSocket-Protocol: iroh-relay-v2, iroh-relay-v1',
        `Authorization: Bearer ${settings.relayAuthToken}`,
        `x-iroh-relay-client-auth-v1: ${auth}`, '', ''].join('\r\n'))
    })
    socket.on('data', chunk => {
      if (ended) return
      buffer = Buffer.concat([buffer, chunk])
      if (!upgraded) {
        const n = buffer.indexOf('\r\n\r\n')
        if (n < 0) return
        const header = buffer.subarray(0, n).toString('ascii')
        result.httpStatus = Number(header.split(' ')[1])
        result.protocol = header.match(/sec-websocket-protocol:\s*([^\r]+)/i)?.[1]
        buffer = buffer.subarray(n + 4)
        if (result.httpStatus !== 101) return finish({errorCategory: 'upgrade_failed'})
        upgraded = true
      }
      while (buffer.length >= 2) {
        let n = buffer[1] & 127, offset = 2
        const op = buffer[0] & 15
        if (n === 126) {
          if (buffer.length < 4) return
          n = buffer.readUInt16BE(2); offset = 4
        }
        if (n === 127 || buffer[1] & 128) return finish({errorCategory: 'unsupported_frame'})
        if (buffer.length < offset + n) return
        const payload = buffer.subarray(offset, offset + n)
        buffer = buffer.subarray(offset + n)
        if (op === 8) return finish({errorCategory: 'closed_by_relay'})
        if (op === 9) { socket.write(frame(payload, 10)); continue }
        if (op !== 2) continue
        if (payload[0] === 2 && !result.authenticated) {
          result.authenticated = true
          result.authReadyMs = performance.now() - start
          ping()
        } else if (payload[0] === 3) return finish({errorCategory: 'authorization_denied'})
        else if (payload[0] === 0) return finish({errorCategory: 'challenge_fallback_not_implemented_by_probe'})
        else if (payload[0] === 10 && payload.subarray(1).equals(nonce)) {
          result.pingPongRttMs.push(performance.now() - pingAt)
          if (result.pingPongRttMs.length === 8) return finish()
          ping()
        }
      }
    })
    socket.on('error', error => finish({errorCategory: error.code || error.name}))
    socket.on('end', () => finish({errorCategory: 'socket_ended'}))
  })
}

console.log(JSON.stringify({kind: 'Mac_raw_relay_auth_ping', observedUtc: new Date().toISOString(),
  scope: 'Node direct TLS API; no file payload, no QUIC, not a DashBeam path proof',
  cases: await Promise.all(settings.relayUrls.map(probe))}, null, 2))
