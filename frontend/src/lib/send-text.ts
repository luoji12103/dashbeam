export const MAX_SEND_TEXT_BYTES = 1024 * 1024

export type TextDraftIssue = 'empty' | 'too-large' | null

export interface TextDraftValidation {
	byteLength: number
	issue: TextDraftIssue
	isValid: boolean
}

export function getTextByteLength(text: string): number {
	return new TextEncoder().encode(text).byteLength
}

/**
 * Do not trim the draft: whitespace and line breaks are part of Markdown text.
 */
export function validateTextDraft(text: string): TextDraftValidation {
	const byteLength = getTextByteLength(text)
	const issue: TextDraftIssue =
		text.length === 0
			? 'empty'
			: byteLength > MAX_SEND_TEXT_BYTES
				? 'too-large'
				: null

	return {
		byteLength,
		issue,
		isValid: issue === null,
	}
}

export function isMarkdownPath(path: string): boolean {
	return /\.md$/i.test(path)
}
