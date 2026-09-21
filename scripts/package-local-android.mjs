#!/usr/bin/env node
// Private local signing identity: never use upstream release credentials.
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import crypto from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const input = process.argv[2]
if (!input || !fs.statSync(input).isFile()) throw new Error('Pass the unsigned arm64 release APK')
const sdk = process.env.ANDROID_HOME
if (!sdk) throw new Error('ANDROID_HOME is required')
const buildTools = path.join(sdk, 'build-tools', '36.0.0')
const signing = path.join(os.homedir(), '.local/share/dashbeam-local-signing')
fs.mkdirSync(signing, { recursive: true, mode: 0o700 })
fs.chmodSync(signing, 0o700)
const passwordFile = path.join(signing, 'password')
const key = path.join(signing, 'release.p12')
if (fs.existsSync(key) && !fs.existsSync(passwordFile)) {
  throw new Error('Signing key exists but its password is missing; restore it, do not replace the identity')
}
if (!fs.existsSync(passwordFile)) {
  fs.writeFileSync(passwordFile, crypto.randomBytes(32).toString('hex'), { flag: 'wx', mode: 0o600 })
}
const env = { ...process.env, DASHBEAM_SIGNING_PASSWORD: fs.readFileSync(passwordFile, 'utf8').trim() }
function run(command, args) {
  const result = spawnSync(command, args, { env, stdio: 'inherit' })
  if (result.error || result.status !== 0) throw new Error(`${path.basename(command)} failed`)
}
if (!fs.existsSync(key)) {
  run('keytool', ['-genkeypair', '-keystore', key, '-storetype', 'PKCS12',
    '-alias', 'dashbeam-local', '-keyalg', 'RSA', '-keysize', '3072', '-validity', '10000',
    '-dname', 'CN=DashBeam Local Build', '-storepass', env.DASHBEAM_SIGNING_PASSWORD])
  fs.chmodSync(key, 0o600)
}
const outputDir = path.join(root, 'dist')
fs.mkdirSync(outputDir, { recursive: true })
const output = path.join(outputDir, 'DashBeam-0.7.1-local-android-arm64.apk')
// Align to 16 KiB boundaries for modern Android devices before signing.
run(path.join(buildTools, 'zipalign'), ['-f', '-P', '16', '4', input, output])
run(path.join(buildTools, 'apksigner'), ['sign', '--ks', key, '--ks-key-alias', 'dashbeam-local',
  '--ks-pass', 'env:DASHBEAM_SIGNING_PASSWORD', output])
run(path.join(buildTools, 'apksigner'), ['verify', '--verbose', '--print-certs', output])
run(path.join(buildTools, 'zipalign'), ['-c', '-P', '16', '4', output])
console.log(`Signed APK: ${output}`)
console.log(`SHA256: ${crypto.createHash('sha256').update(fs.readFileSync(output)).digest('hex')}`)
