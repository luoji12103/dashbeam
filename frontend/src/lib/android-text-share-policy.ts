import { validateTextDraft } from './send-text.js'

export type AndroidTextShareAction =
	| 'apply'
	| 'confirm-replace'
	| 'defer'
	| 'reject'

export interface AndroidTextShareDecision {
	action: AndroidTextShareAction
	issue?: 'empty' | 'too-large'
}

/**
 * Android's share sheet can arrive at any point in the app lifecycle. Keep the
 * decision pure so a new text intent cannot replace a draft or alter a live
 * transfer merely because the WebView resumed.
 */
export function decideAndroidTextShare({
	text,
	draft,
	isTransferActive,
}: {
	text: string
	draft: string
	isTransferActive: boolean
}): AndroidTextShareDecision {
	if (isTransferActive) {
		return { action: 'defer' }
	}

	const validation = validateTextDraft(text)
	if (!validation.isValid) {
		return { action: 'reject', issue: validation.issue ?? undefined }
	}

	return draft.length > 0 ? { action: 'confirm-replace' } : { action: 'apply' }
}
