import assert from 'node:assert/strict'
import { test } from 'node:test'
import { useSenderStore } from './sender-store.js'

test('fast sending is an explicit choice for the current file only', () => {
	const store = useSenderStore
	store.getState().resetToIdle()
	store.getState().setSelectedPaths(['/tmp/first.bin'])
	store.getState().setPathType('file')
	store.getState().setSourceStream(true)
	assert.equal(store.getState().sourceStream, true)

	// Adding a file must not silently broaden the fast mode to a collection.
	store.getState().addSelectedPaths(['/tmp/second.bin'])
	assert.equal(store.getState().sourceStream, false)
	store.getState().removeSelectedPath('/tmp/second.bin')
	assert.equal(store.getState().sourceStream, false)

	store.getState().setSourceStream(true)
	store.getState().setSendMode('text')
	assert.equal(store.getState().sourceStream, false)
	store.getState().setSendMode('file')
	store.getState().setSourceStream(true)
	store.getState().setPathType('directory')
	assert.equal(store.getState().sourceStream, false)

	store.getState().setSourceStream(true)
	store.getState().resetToIdle()
	assert.equal(store.getState().sourceStream, false)
})
