export const MAX_RECEIVED_TEXT_BYTES = 1024 * 1024

export interface ReceivedTextReadyPayload {
	ticket: string
	path: string
	size: number
}

export function isMarkedTextMetadata(metadata: {
	content_kind?: unknown
}): boolean {
	return metadata.content_kind === 'text'
}

function parseReceivedTextPayload(
	payload: unknown
): ReceivedTextReadyPayload | null {
	try {
		const value =
			typeof payload === 'string'
				? (JSON.parse(payload) as Record<string, unknown>)
				: (payload as Record<string, unknown> | null)
		if (!value || typeof value !== 'object') return null

		const ticket = typeof value.ticket === 'string' ? value.ticket.trim() : ''
		const path = typeof value.path === 'string' ? value.path : ''
		const size = typeof value.size === 'number' ? value.size : -1
		if (
			!ticket ||
			!path ||
			!Number.isSafeInteger(size) ||
			size < 0 ||
			size > MAX_RECEIVED_TEXT_BYTES
		) {
			return null
		}
		return { ticket, path, size }
	} catch {
		return null
	}
}

export function parseReceivedTextReady(
	payload: unknown,
	activeTicket: string
): ReceivedTextReadyPayload | null {
	const parsed = parseReceivedTextPayload(payload)
	return parsed?.ticket === activeTicket ? parsed : null
}

/** Validates a Rust-side recovery record without requiring a live transfer. */
export function parsePendingReceivedText(
	payload: unknown
): ReceivedTextReadyPayload | null {
	return parseReceivedTextPayload(payload)
}
