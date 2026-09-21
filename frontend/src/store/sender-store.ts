import { create } from 'zustand'
import type { TransferMetadata, TransferProgress } from '../types/transfer'
import type { AlertDialogState, AlertType } from '../types/ui'

// Define explicit view states for predictable UI rendering
export type SenderViewState = 'IDLE' | 'SHARING' | 'TRANSPORTING' | 'SUCCESS'
export type SenderMode = 'file' | 'text'

export interface SenderStore {
	// View state (replaces isSharing, isTransporting, isCompleted)
	viewState: SenderViewState

	// Transfer data
	ticket: string | null
	selectedPaths: string[]
	selectedPath: string | null
	pathType: 'file' | 'directory' | null
	thumbnailUrl: string | null
	transferMetadata: TransferMetadata | null
	transferProgress: TransferProgress | null
	sendMode: SenderMode
	/** Runtime-only text draft. It must never be written to settings or history. */
	textDraft: string

	// UI flags
	isLoading: boolean
	copySuccess: boolean
	isBroadcastMode: boolean
	alertDialog: AlertDialogState
	activeConnectionCount: number
	/** True after Sender has subscribed to native transfer lifecycle events. */
	transferEventListenersReady: boolean

	// Actions
	setViewState: (state: SenderViewState) => void
	setTicket: (ticket: string | null) => void
	setSelectedPaths: (paths: string[]) => void
	addSelectedPaths: (paths: string[]) => void
	removeSelectedPath: (path: string) => void
	setSelectedPath: (path: string | null) => void
	setPathType: (type: 'file' | 'directory' | null) => void
	setThumbnailUrl: (url: string | null) => void
	setTransferMetadata: (metadata: TransferMetadata | null) => void
	setTransferProgress: (progress: TransferProgress | null) => void
	setSendMode: (mode: SenderMode) => void
	setTextDraft: (text: string) => void
	clearTextDraft: () => void
	setIsLoading: (loading: boolean) => void
	setCopySuccess: (success: boolean) => void
	setIsBroadcastMode: (enabled: boolean) => void
	toggleBroadcastMode: () => void
	setAlertDialog: (dialog: AlertDialogState) => void
	setActiveConnectionCount: (count: number) => void
	setTransferEventListenersReady: (ready: boolean) => void
	showAlert: (title: string, description: string, type?: AlertType) => void
	closeAlert: () => void

	// Complex state transitions
	resetToIdle: () => void
	resetForBroadcast: () => void
}

export const useSenderStore = create<SenderStore>()((set) => ({
	// Initial state
	viewState: 'IDLE',
	ticket: null,
	selectedPaths: [],
	selectedPath: null,
	pathType: null,
	thumbnailUrl: null,
	transferMetadata: null,
	transferProgress: null,
	sendMode: 'file',
	textDraft: '',
	isLoading: false,
	copySuccess: false,
	isBroadcastMode: false,
	activeConnectionCount: 0,
	transferEventListenersReady: false,
	alertDialog: {
		isOpen: false,
		title: '',
		description: '',
		type: 'info',
	},

	// Actions — simple setters
	setViewState: (viewState) => {
		set({ viewState })
	},
	setTicket: (ticket) => set({ ticket }),
	setSelectedPaths: (selectedPaths) =>
		set({
			selectedPaths,
			selectedPath: selectedPaths[0] ?? null,
		}),
	addSelectedPaths: (paths) =>
		set((state) => {
			const deduped = new Set(state.selectedPaths)
			for (const path of paths) {
				deduped.add(path)
			}
			const selectedPaths = Array.from(deduped)
			return {
				selectedPaths,
				selectedPath: selectedPaths[0] ?? null,
			}
		}),
	removeSelectedPath: (path) =>
		set((state) => {
			const selectedPaths = state.selectedPaths.filter((item) => item !== path)
			return {
				selectedPaths,
				selectedPath: selectedPaths[0] ?? null,
				pathType: selectedPaths.length ? state.pathType : null,
			}
		}),
	setSelectedPath: (selectedPath) =>
		set({ selectedPath, selectedPaths: selectedPath ? [selectedPath] : [] }),
	setPathType: (pathType) => set({ pathType }),
	setThumbnailUrl: (thumbnailUrl) => set({ thumbnailUrl }),
	setTransferMetadata: (transferMetadata) => {
		set({ transferMetadata })
	},
	setTransferProgress: (transferProgress) => set({ transferProgress }),
	setSendMode: (sendMode) => set({ sendMode }),
	setTextDraft: (textDraft) => set({ textDraft }),
	clearTextDraft: () => set({ textDraft: '' }),
	setIsLoading: (isLoading) => set({ isLoading }),
	setCopySuccess: (copySuccess) => set({ copySuccess }),
	setIsBroadcastMode: (isBroadcastMode) => set({ isBroadcastMode }),
	toggleBroadcastMode: () =>
		set((state) => ({ isBroadcastMode: !state.isBroadcastMode })),
	setAlertDialog: (alertDialog) => set({ alertDialog }),
	setActiveConnectionCount: (activeConnectionCount) =>
		set({ activeConnectionCount }),
	setTransferEventListenersReady: (transferEventListenersReady) =>
		set({ transferEventListenersReady }),

	showAlert: (title, description, type = 'info') =>
		set({
			alertDialog: {
				isOpen: true,
				title,
				description,
				type,
			},
		}),

	closeAlert: () =>
		set((state) => ({
			alertDialog: {
				...state.alertDialog,
				isOpen: false,
			},
		})),

	// Complex state transitions
	resetToIdle: () => {
		set({
			viewState: 'IDLE',
			ticket: null,
			selectedPaths: [],
			selectedPath: null,
			pathType: null,
			thumbnailUrl: null,
			transferMetadata: null,
			transferProgress: null,
			isLoading: false,
			isBroadcastMode: false,
			activeConnectionCount: 0,
		})
	},

	resetForBroadcast: () => {
		set({
			viewState: 'SHARING',
			transferMetadata: null,
			transferProgress: null,
			thumbnailUrl: null,
			activeConnectionCount: 0,
		})
	},
}))
