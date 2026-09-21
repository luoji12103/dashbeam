import type { AlertDialogState } from './ui'
import type { TicketPreviewMetadata } from './transfer'

export interface ReceiverState {
	ticket: string
	isReceiving: boolean
	alertDialog: AlertDialogState
}

export interface ReceivedTextState {
	resultId: string
	content: string
	size: number
	path: string
	isCopied: boolean
	isCopying: boolean
	copyError: string | null
}

export interface TicketInputProps {
	ticket: string
	isReceiving: boolean
	previewMetadata: TicketPreviewMetadata | null
	isPreviewLoading: boolean
	onTicketChange: (ticket: string) => void
	onReceive: () => Promise<void>
}
