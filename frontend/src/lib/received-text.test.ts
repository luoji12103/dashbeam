import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
	isMarkedTextMetadata,
	MAX_RECEIVED_TEXT_BYTES,
	parsePendingReceivedText,
	parseReceivedTextReady,
} from './received-text.js'

describe('received text eligibility', () => {
	it('requires the explicit text marker', () => {
		const ordinaryTextFile = {
			content_kind: undefined,
			file_name: 'note.txt',
		}
		assert.equal(isMarkedTextMetadata({ content_kind: 'text' }), true)
		assert.equal(isMarkedTextMetadata({ content_kind: 'file' }), false)
		assert.equal(isMarkedTextMetadata({}), false)
		assert.equal(isMarkedTextMetadata(ordinaryTextFile), false)
	})
})

describe('parseReceivedTextReady', () => {
	const ticket = 'blob-ticket'

	it('accepts the matching completed text event', () => {
		assert.deepEqual(
			parseReceivedTextReady(
				JSON.stringify({ ticket, path: '/tmp/message.txt', size: 5 }),
				ticket
			),
			{ ticket, path: '/tmp/message.txt', size: 5 }
		)
	})

	it('rejects malformed and stale events', () => {
		assert.equal(parseReceivedTextReady('{', ticket), null)
		assert.equal(
			parseReceivedTextReady(
				{ ticket: 'old-ticket', path: '/tmp/message.txt', size: 5 },
				ticket
			),
			null
		)
		assert.equal(
			parseReceivedTextReady({ ticket, path: '', size: 5 }, ticket),
			null
		)
	})

	it('enforces the byte cap', () => {
		assert.ok(
			parseReceivedTextReady(
				{ ticket, path: '/tmp/message.txt', size: MAX_RECEIVED_TEXT_BYTES },
				ticket
			)
		)
		assert.equal(
			parseReceivedTextReady(
				{
					ticket,
					path: '/tmp/message.txt',
					size: MAX_RECEIVED_TEXT_BYTES + 1,
				},
				ticket
			),
			null
		)
	})

	it('accepts a validated recovery record without a live ticket', () => {
		assert.deepEqual(
			parsePendingReceivedText({
				ticket,
				path: '/tmp/message.txt',
				size: 5,
			}),
			{ ticket, path: '/tmp/message.txt', size: 5 }
		)
	})
})
