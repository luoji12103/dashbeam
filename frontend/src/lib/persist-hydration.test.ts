import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { afterPersistHydration } from './persist-hydration.js'

function hydrationSource(initiallyHydrated = false) {
	let hydrated = initiallyHydrated
	const listeners = new Set<() => void>()
	return {
		hasHydrated: () => hydrated,
		onFinishHydration: (listener: () => void) => {
			listeners.add(listener)
			return () => listeners.delete(listener)
		},
		finish: () => {
			hydrated = true
			for (const listener of listeners) listener()
		},
		listenerCount: () => listeners.size,
	}
}

describe('afterPersistHydration', () => {
	it('never publishes default settings while persisted settings are loading', () => {
		const source = hydrationSource()
		let relay = 'default'
		const published: string[] = []
		afterPersistHydration(source, () => published.push(relay))
		assert.deepEqual(published, [])
		relay = 'custom'
		source.finish()
		source.finish()
		assert.deepEqual(published, ['custom'])
		assert.equal(source.listenerCount(), 0)
	})

	it('runs immediately when settings hydrated before the node became ready', () => {
		const source = hydrationSource(true)
		let calls = 0
		afterPersistHydration(source, () => calls++)
		assert.equal(calls, 1)
		assert.equal(source.listenerCount(), 0)
	})

	it('cancels a pending sync when its owner unmounts or the node becomes unavailable', () => {
		const source = hydrationSource()
		let calls = 0
		const stop = afterPersistHydration(source, () => calls++)
		stop()
		source.finish()
		assert.equal(calls, 0)
		assert.equal(source.listenerCount(), 0)
	})

	it('does not miss hydration between subscribing and checking readiness', () => {
		let listener: (() => void) | undefined
		let calls = 0
		afterPersistHydration(
			{
				onFinishHydration: (next) => {
					listener = next
					return () => {
						listener = undefined
					}
				},
				hasHydrated: () => {
					listener?.()
					return true
				},
			},
			() => calls++
		)
		assert.equal(calls, 1)
		assert.equal(listener, undefined)
	})
})
