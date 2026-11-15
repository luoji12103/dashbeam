import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { prepareShare, prepareTargetedShare } from './share-preparation.js'

function deferred<T>() {
	let resolve!: (value: T) => void
	let reject!: (reason: Error) => void
	const promise = new Promise<T>((done, fail) => {
		resolve = done
		reject = fail
	})
	return { promise, resolve, reject }
}

describe('prepareShare', () => {
	it('does no preparation before a target and releases a cancelled target gate', async () => {
		let preparations = 0
		const waiting = prepareTargetedShare({
			prepare: async () => {
				preparations++
				return 'ticket'
			},
			dispose: async () => {},
		})
		await Promise.resolve()
		assert.equal(preparations, 0)
		waiting.cancel()
		assert.equal(waiting.selectTarget?.('old-peer'), false)
		assert.equal(await waiting.result, null)
		const replacement = prepareShare({
			previous: waiting.result,
			prepare: async () => {
				preparations++
				return 'replacement'
			},
			dispose: async () => {},
		})
		assert.equal(await replacement.result, 'replacement')
		assert.equal(preparations, 1)
	})

	it('prepares once for the first selected target and rejects a second selection', async () => {
		const targets: string[] = []
		const share = prepareTargetedShare({
			prepare: async (target) => {
				targets.push(target)
				return 'legacy-ticket'
			},
			dispose: async () => {},
		})
		assert.equal(share.selectTarget?.('old-client'), true)
		assert.equal(share.selectTarget?.('other-client'), false)
		assert.equal(await share.result, 'legacy-ticket')
		assert.deepEqual(targets, ['old-client'])
	})

	it('does not prepare when target selection and cancellation happen in the same tick', async () => {
		let preparations = 0
		const share = prepareTargetedShare({
			prepare: async () => {
				preparations++
				return 'ticket'
			},
			dispose: async () => {},
		})
		await Promise.resolve()
		share.selectTarget?.('peer')
		share.cancel()
		assert.equal(await share.result, null)
		assert.equal(preparations, 0)
	})

	it('cleans up the actual replacement ticket when cancellation races with fallback', async () => {
		const prepared = deferred<string>()
		const ticketRef = { current: 'stream-ticket' }
		const disposed: string[] = []
		const share = prepareTargetedShare({
			prepare: () => prepared.promise,
			dispose: async (ticket) => {
				disposed.push(ticketRef.current || ticket)
			},
		})
		share.selectTarget?.('peer')
		await Promise.resolve()
		await Promise.resolve()
		share.cancel()
		ticketRef.current = 'actual-legacy-ticket'
		prepared.resolve('original-ticket')
		assert.equal(await share.result, null)
		assert.deepEqual(disposed, ['actual-legacy-ticket'])
	})

	it('disposes a late ticket after cancellation before starting its replacement', async () => {
		const prepared = deferred<string>()
		const cleaned = deferred<void>()
		const cleanupStarted = deferred<void>()
		const events: string[] = []
		const first = prepareShare({
			prepare: async () => {
				events.push('first start')
				return prepared.promise
			},
			dispose: async (ticket) => {
				assert.equal(ticket, 'first ticket')
				events.push('first cleanup')
				cleanupStarted.resolve()
				await cleaned.promise
			},
		})
		await Promise.resolve()
		first.cancel()
		const second = prepareShare({
			previous: first.result,
			prepare: async () => {
				events.push('second start')
				return 'second ticket'
			},
			dispose: async () => {},
		})
		prepared.resolve('first ticket')
		await cleanupStarted.promise
		assert.deepEqual(events, ['first start', 'first cleanup'])
		cleaned.resolve()
		assert.equal(await first.result, null)
		assert.equal(await second.result, 'second ticket')
		assert.deepEqual(events, ['first start', 'first cleanup', 'second start'])
	})

	it('does not invoke a queued preparation cancelled before it starts', async () => {
		const previous = deferred<void>()
		let calls = 0
		const share = prepareShare({
			previous: previous.promise,
			prepare: async () => {
				calls++
				return 'ticket'
			},
			dispose: async () => {},
		})
		share.cancel()
		previous.resolve()
		assert.equal(await share.result, null)
		assert.equal(calls, 0)
	})

	it('suppresses a backend cancellation rejection but preserves a live failure', async () => {
		const backend = deferred<string>()
		const cancelled = prepareShare({
			prepare: () => backend.promise,
			dispose: async () => {},
		})
		await Promise.resolve()
		cancelled.cancel()
		backend.reject(new Error('cancelled'))
		assert.equal(await cancelled.result, null)
		const failed = prepareShare({
			prepare: async () => {
				throw new Error('source unavailable')
			},
			dispose: async () => {},
		})
		await assert.rejects(failed.result, /source unavailable/)
	})

	it('blocks a replacement if a cancelled share cannot be cleaned up', async () => {
		const backend = deferred<string>()
		const first = prepareShare({
			prepare: () => backend.promise,
			dispose: async () => {
				throw new Error('cleanup failed')
			},
		})
		await Promise.resolve()
		first.cancel()
		let started = false
		const second = prepareShare({
			previous: first.result,
			prepare: async () => {
				started = true
				return 'ticket'
			},
			dispose: async () => {},
		})
		backend.resolve('late ticket')
		await assert.rejects(first.result, /cleanup failed/)
		await assert.rejects(second.result, /cleanup failed/)
		assert.equal(started, false)
	})
})
