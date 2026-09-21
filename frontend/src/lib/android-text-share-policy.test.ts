import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { MAX_SEND_TEXT_BYTES } from './send-text.js'
import { decideAndroidTextShare } from './android-text-share-policy.js'

describe('decideAndroidTextShare', () => {
	it('defers a system text share while a transfer is active', () => {
		assert.deepEqual(
			decideAndroidTextShare({
				text: 'incoming',
				draft: '',
				isTransferActive: true,
			}),
			{ action: 'defer' }
		)
	})

	it('does not replace an existing draft without confirmation', () => {
		assert.deepEqual(
			decideAndroidTextShare({
				text: 'incoming',
				draft: 'keep this draft',
				isTransferActive: false,
			}),
			{ action: 'confirm-replace' }
		)
	})

	it('applies a valid share to an empty draft and rejects invalid text', () => {
		assert.deepEqual(
			decideAndroidTextShare({
				text: '## Shared Markdown',
				draft: '',
				isTransferActive: false,
			}),
			{ action: 'apply' }
		)
		assert.deepEqual(
			decideAndroidTextShare({
				text: '',
				draft: '',
				isTransferActive: false,
			}),
			{ action: 'reject', issue: 'empty' }
		)
		assert.deepEqual(
			decideAndroidTextShare({
				text: 'x'.repeat(MAX_SEND_TEXT_BYTES + 1),
				draft: '',
				isTransferActive: false,
			}),
			{ action: 'reject', issue: 'too-large' }
		)
	})
})
