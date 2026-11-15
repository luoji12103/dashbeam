// Two bounded temporary peers. Network JSON comes from stdin only.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { chmodSync, copyFileSync, mkdtempSync, readdirSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { createInterface } from 'node:readline'

const [binaryArg, parentArg] = process.argv.slice(2)
if (!binaryArg || !parentArg) {
	throw new Error('Usage: node device_probe_smoke.mjs /absolute/device_probe /existing/task-parent')
}
const input = createInterface({ input: process.stdin })
const network = await new Promise((resolveLine, reject) => {
	const timer = setTimeout(() => reject(new Error('network configuration stdin timeout')), 10_000)
	input.once('line', (line) => {
		clearTimeout(timer)
		input.close()
		resolveLine(line)
	})
})
JSON.parse(network)
const root = mkdtempSync(join(resolve(parentArg), 'probe-smoke-'))
chmodSync(root, 0o700)

function peer(name) {
	const path = join(root, name)
	const child = spawn(resolve(binaryArg), ['--work-dir', path, '--name', `Temporary probe ${name}`, '--ttl-secs', '300'], {
		stdio: ['pipe', 'pipe', 'pipe'],
	})
	const messages = []
	const waiters = new Set()
	const lines = createInterface({ input: child.stdout })
	let exited = false
	let stderrPresent = false
	child.stderr.on('data', () => { stderrPresent = true })
	lines.on('line', (line) => {
		let event
		try { event = JSON.parse(line) } catch { return }
		messages.push(event)
		for (const waiter of waiters) waiter()
	})
	child.once('exit', () => {
		exited = true
		for (const waiter of waiters) waiter()
	})
	function wait(eventName, after = 0, predicate = () => true, timeout = 90_000) {
		return new Promise((resolveEvent, reject) => {
			const timer = setTimeout(() => finish(new Error(`${name}: timeout waiting for ${eventName}`)), timeout)
			function finish(error, event) {
				clearTimeout(timer)
				waiters.delete(check)
				if (error) reject(error)
				else resolveEvent(event)
			}
			function check() {
				const event = messages.slice(after).find((event) => event.event === eventName && predicate(event))
				if (event) return finish(null, event)
				const failure = messages.slice(after).find((event) => event.event === 'fatal' || event.event === 'command-error')
				if (failure) return finish(new Error(`${name}: ${failure.event} ${failure.op ?? ''} ${failure.kind}`))
				if (exited) finish(new Error(`${name}: exited before ${eventName}; stderr=${stderrPresent}`))
			}
			waiters.add(check)
			check()
		})
	}
	function send(command) {
		const mark = messages.length
		child.stdin.write(`${JSON.stringify(command)}\n`)
		return mark
	}
	child.stdin.write(`${network}\n`)
	return { name, path, child, messages, wait, send }
}

const a = peer('a')
const b = peer('b')
const results = []
try {
	await Promise.all([a.wait('ready'), b.wait('ready')])
	await Promise.all([a, b].map(async (value) => {
		for (let attempt = 0; attempt < 45; attempt++) {
			const status = await value.wait('status', value.send({ op: 'status' }))
			if (status.node_alive) return
			await new Promise((resolveDelay) => setTimeout(resolveDelay, 2_000))
		}
		throw new Error(`${value.name}: network not ready after status queries`)
	}))
	const pairing = await a.wait('pairing-host', a.send({ op: 'host' }))
	copyFileSync(join(a.path, pairing.ticket_file), join(b.path, 'peer.ticket'))
	chmodSync(join(b.path, 'peer.ticket'), 0o600)
	await b.wait('pairing-joined', b.send({ op: 'join', ticket_file: 'peer.ticket' }))
	await a.wait('device-paired')

	async function transfer(sender, receiver, kind, fixture, cancelAfterMs) {
		const armed = { op: 'arm', fixture }
		if (cancelAfterMs !== undefined) armed.cancel_after_ms = cancelAfterMs
		const receiveMark = receiver.send(armed)
		await receiver.wait('armed', receiveMark)
		const sent = await sender.wait('share-invited', sender.send({ op: 'send', kind, fixture }))
		assert.equal(sent.delivered, true)
		if (kind === 'stream') assert.equal(sent.zero_prepare_reads, true)
		const received = await receiver.wait('receive-result', receiveMark)
		if (cancelAfterMs !== undefined) {
			assert.equal(received.ok, false)
			assert.equal(received.kind, 'cancelled')
			assert.equal(received.partial_clean, true)
		} else {
			assert.equal(received.ok, true)
		}
		assert.equal(received.node_alive, true)
		const stopped = await sender.wait('share-stopped', sender.send({ op: 'stop' }))
		assert.equal(stopped.node_alive, true)
		const result = { direction: `${sender.name}->${receiver.name}`, kind, fixture,
			prepare_us: sent.prepare_us, zero_prepare_reads: sent.zero_prepare_reads, ...received }
		results.push(result)
		console.log(JSON.stringify({ event: 'case-passed', direction: result.direction, kind, fixture,
			cancelled: cancelAfterMs !== undefined, elapsed_ms: received.elapsed_ms }))
	}
	await transfer(a, b, 'legacy', 'small')
	await transfer(a, b, 'legacy', 'multi')
	await transfer(a, b, 'text', 'text')
	await transfer(a, b, 'stream', 'empty')
	await transfer(a, b, 'stream', 'small')
	for (let i = 0; i < 3; i++) await transfer(a, b, 'stream', 'large')
	await transfer(a, b, 'stream', 'cancel', 1)
	await transfer(a, b, 'stream', 'small')
	await transfer(b, a, 'stream', 'small')
	await transfer(b, a, 'legacy', 'small')
	await transfer(b, a, 'text', 'text')
	await a.wait('peer-forgotten', a.send({ op: 'forget' }))
	const stopped = [a.wait('stopped', a.send({ op: 'quit' })), b.wait('stopped', b.send({ op: 'quit' }))]
	await Promise.all(stopped)
	for (const value of [a, b]) {
		assert.equal(readdirSync(value.path).some((name) => name.startsWith('identity-') || name.startsWith('stores-')), false)
	}
	writeFileSync(join(root, 'receipts.json'), `${JSON.stringify(results, null, 2)}\n`, { mode: 0o600, flag: 'wx' })
	console.log(JSON.stringify({ event: 'smoke-summary', passed: results.length, cancelled: 1, identity_cleaned: true, root }))
} finally {
	for (const value of [a, b]) {
		if (value.child.exitCode === null) {
			value.child.stdin.write('{"op":"quit"}\n')
			setTimeout(() => { if (value.child.exitCode === null) value.child.kill('SIGTERM') }, 15_000).unref()
		}
	}
}
