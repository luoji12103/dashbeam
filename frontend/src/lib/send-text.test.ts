import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
	getTextByteLength,
	isMarkdownPath,
	MAX_SEND_TEXT_BYTES,
	validateTextDraft,
} from './send-text.js'

describe('validateTextDraft', () => {
	it('keeps Markdown whitespace untouched while validating it', () => {
		const draft = '  # Heading\n\n- one\n- two  \n'

		assert.deepEqual(validateTextDraft(draft), {
			byteLength: getTextByteLength(draft),
			issue: null,
			isValid: true,
		})
	})

	it('rejects only an actually empty draft', () => {
		assert.equal(validateTextDraft('').issue, 'empty')
		assert.equal(validateTextDraft('\n  ').isValid, true)
	})

	it('counts UTF-8 bytes and enforces the 1 MiB limit', () => {
		assert.equal(getTextByteLength('A中🙂'), 8)
		assert.equal(
			validateTextDraft('a'.repeat(MAX_SEND_TEXT_BYTES)).isValid,
			true
		)
		assert.equal(
			validateTextDraft('a'.repeat(MAX_SEND_TEXT_BYTES + 1)).issue,
			'too-large'
		)
	})
})

describe('isMarkdownPath', () => {
	it('accepts only Markdown file paths for text import', () => {
		assert.equal(isMarkdownPath('/tmp/notes.md'), true)
		assert.equal(isMarkdownPath('/tmp/NOTES.MD'), true)
		assert.equal(isMarkdownPath('/tmp/notes.md.bak'), false)
		assert.equal(isMarkdownPath('/tmp/notes.txt'), false)
	})
})
