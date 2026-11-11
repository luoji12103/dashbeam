import { useCallback, useEffect, useRef, useState } from 'react'
import { useNodeCapability } from '@/hooks/useNodeCapability'
import {
	applyPresencePatch,
	usePairedDeviceEvents,
} from '@/hooks/usePairedDeviceEvents'
import { incrementPairedSendCount } from '@/lib/paired-send-counts'
import {
	inviteNearbyDevice,
	invitePairedDevice,
	isPairedDeviceActive,
	listPairedDevices,
	type PairedDevice,
} from '@/lib/pairing-api'
import { IS_PAIRING_CAPABLE } from '@/lib/platform'
import { invoke, listen, type UnlistenFn } from '@/lib/platform-api'
import {
	getWebPreviewErrorMessage,
	isWebPreviewError,
} from '@/lib/web-preview-error'
import { useNearbyStore } from '@/store/nearby-store'
import { useNearbyVerificationStore } from '@/store/nearby-verification-store'
import { toastManager } from '../components/ui/toast'
import { useTranslation } from '../i18n/react-i18next-compat'
import { getDiscoveryConfigArg } from '../lib/discovery'
import { getRelayConfigArg } from '../lib/relay'
import { getTextByteLength, validateTextDraft } from '../lib/send-text'
import { prepareShare, type SharePreparation } from '../lib/share-preparation'
import {
	parseCompletionPayload,
	parseProgressPayload,
} from '../lib/transfer-events'
import { copyTextToClipboard } from '../lib/utils'
import { useSenderStore } from '../store/sender-store'
import type { TransferMetadata, TransferProgress } from '../types/transfer'
import type { AlertType } from '../types/ui'

export type PairedInviteStatus = 'preparing' | 'sending' | 'sent' | 'failed'

type PreparedShare = {
	generation: number
	preparation: SharePreparation<string>
	sendMode: 'file' | 'text'
	textDraft: string
	selectedPaths: string[]
	totalSize?: Promise<number>
}

// Sender unmounts when navigating to Settings. Preparation, cancellation and
// replacement ordering belong to the app session, rather than that view.
const shareGenerationRef = { current: 0 }
const preparedShareRef: { current: PreparedShare | null } = { current: null }
const pendingStopRef: { current: Promise<unknown> | undefined } = {
	current: undefined,
}
const inviteInFlightRef: { current: string | null } = { current: null }
const totalSizeCacheRef: {
	current: { ticket: string; size: Promise<number> } | null
} = { current: null }

export interface UseSenderReturn {
	// View state (replaces isSharing, isTransporting, isCompleted)
	viewState: 'IDLE' | 'SHARING' | 'TRANSPORTING' | 'SUCCESS'

	// Derived states for backward compatibility
	isSharing: boolean
	isTransporting: boolean
	isCompleted: boolean

	ticket: string | null
	selectedPaths: string[]
	selectedPath: string | null
	pathType: 'file' | 'directory' | null
	isLoading: boolean
	copySuccess: boolean
	alertDialog: any
	transferMetadata: TransferMetadata | null
	transferProgress: TransferProgress | null
	sendMode: 'file' | 'text'
	textDraft: string
	isBroadcastMode: boolean
	activeConnectionCount: number
	pairedDevices: PairedDevice[]
	isNodeReady: boolean
	isNodeStatusPending?: boolean
	pairedInviteStatus: Record<string, PairedInviteStatus>
	onInvitePairedDevice: (endpointId: string) => Promise<boolean>
	onInviteNearbyDevice: (endpointId: string) => Promise<boolean>

	handleFileSelect: (
		path: string,
		pathType?: 'file' | 'directory'
	) => Promise<void>
	handleFilesSelect: (
		paths: string[],
		pathType?: 'file' | 'directory'
	) => Promise<void>
	clearSelectedPath: () => void
	removeSelectedPath: (path: string) => void
	setSendMode: (mode: 'file' | 'text') => void
	setTextDraft: (text: string) => void
	clearTextDraft: () => void
	startSharing: () => Promise<void>
	stopSharing: () => Promise<void>
	copyTicket: () => Promise<void>
	showAlert: (title: string, description: string, type?: AlertType) => void
	closeAlert: () => void
	resetForNewTransfer: () => Promise<void>
}

export function useSender(): UseSenderReturn {
	const { t } = useTranslation()

	// Get store state and actions
	const {
		viewState,
		ticket,
		selectedPaths,
		selectedPath,
		pathType,
		isLoading,
		copySuccess,
		alertDialog,
		transferMetadata,
		transferProgress,
		sendMode,
		textDraft,
		isBroadcastMode,
		activeConnectionCount,
		setViewState,
		setTicket,
		setSelectedPaths,
		addSelectedPaths,
		removeSelectedPath: removeSelectedPathFromStore,
		setPathType,
		setIsLoading,
		setCopySuccess,
		setTransferMetadata,
		setTransferProgress,
		setSendMode,
		setTextDraft,
		clearTextDraft,
		setIsBroadcastMode,
		showAlert,
		closeAlert,
		resetToIdle,
		resetForBroadcast,
		setActiveConnectionCount,
	} = useSenderStore()

	const [pairedDevices, setPairedDevices] = useState<PairedDevice[]>([])
	const [pairedInviteStatus, setPairedInviteStatus] = useState<
		Record<string, PairedInviteStatus>
	>({})
	const { isNodeReady, isNodeStatusPending } = useNodeCapability()

	const setInviteStatus = useCallback(
		(endpointId: string, status: PairedInviteStatus | null) => {
			setPairedInviteStatus((prev) => {
				if (status === null) {
					if (!(endpointId in prev)) return prev
					const next = { ...prev }
					delete next[endpointId]
					return next
				}
				return { ...prev, [endpointId]: status }
			})
		},
		[]
	)

	const refreshPairedDevices = useCallback(async () => {
		if (!IS_PAIRING_CAPABLE) {
			setPairedDevices([])
			return
		}
		try {
			setPairedDevices(await listPairedDevices())
		} catch (error) {
			console.error('Failed to load paired devices:', error)
		}
	}, [])

	useEffect(() => {
		void refreshPairedDevices()
	}, [refreshPairedDevices])

	useEffect(() => {
		if (!ticket) {
			setPairedInviteStatus({})
		}
	}, [ticket])

	usePairedDeviceEvents({
		onPresence: useCallback((payload) => {
			applyPresencePatch(setPairedDevices, payload)
		}, []),
		onUnpaired: useCallback(() => {}, []),
		onRefresh: refreshPairedDevices,
	})

	// Refs for event listeners
	const latestProgressRef = useRef<TransferProgress | null>(null)
	const transferStartTimeRef = useRef<number | null>(null)
	const progressUpdateIntervalRef = useRef<ReturnType<
		typeof setInterval
	> | null>(null)
	const wasManuallyStoppedRef = useRef(false)
	const selectedPathRef = useRef<string | null>(null)
	const pathTypeRef = useRef<'file' | 'directory' | null>(null)

	useEffect(() => {
		selectedPathRef.current = selectedPath
	}, [selectedPath])

	useEffect(() => {
		pathTypeRef.current = pathType
	}, [pathType])

	useEffect(() => {
		let disposed = false
		let unlistenStart: UnlistenFn | undefined
		let unlistenProgress: UnlistenFn | undefined
		let unlistenComplete: UnlistenFn | undefined
		let unlistenFailed: UnlistenFn | undefined
		let unlistenActiveCount: UnlistenFn | undefined
		let unlistenDevicePaired: UnlistenFn | undefined
		let unlistenInviteResponse: UnlistenFn | undefined

		const safeUnlisten = (unlisten?: UnlistenFn) => {
			if (unlisten) {
				unlisten()
			}
		}

		const setupListeners = async () => {
			const nextUnlistenActiveCount = await listen(
				'active-connection-count',
				(event: any) => {
					try {
						const count = parseInt(event.payload as string, 10)
						if (!Number.isNaN(count)) {
							setActiveConnectionCount(count)
						}
					} catch (error) {
						console.error(
							'Failed to parse active connection count event:',
							error
						)
					}
				}
			)
			if (disposed) {
				nextUnlistenActiveCount()
			} else {
				unlistenActiveCount = nextUnlistenActiveCount
			}

			const nextUnlistenDevicePaired = await listen('device-paired', () => {
				void refreshPairedDevices()
			})
			if (disposed) {
				nextUnlistenDevicePaired()
			} else {
				unlistenDevicePaired = nextUnlistenDevicePaired
			}

			const nextUnlistenInviteResponse = await listen(
				'paired-invite-response',
				(event: { payload: unknown }) => {
					try {
						const payload =
							typeof event.payload === 'string'
								? (JSON.parse(event.payload) as {
										endpoint_id: string
										response: string
									})
								: (event.payload as {
										endpoint_id: string
										response: string
									})
						if (!payload?.endpoint_id) return
						if (inviteInFlightRef.current === payload.endpoint_id) {
							inviteInFlightRef.current = null
						}
						setInviteStatus(payload.endpoint_id, null)
					} catch {
						// Ignore malformed invite response payloads
					}
				}
			)
			if (disposed) {
				nextUnlistenInviteResponse()
			} else {
				unlistenInviteResponse = nextUnlistenInviteResponse
			}

			const nextUnlistenStart = await listen('transfer-started', () => {
				const storeState = useSenderStore.getState()
				if (
					storeState.viewState === 'IDLE' ||
					storeState.isLoading ||
					!storeState.ticket
				) {
					return
				}

				transferStartTimeRef.current = Date.now()
				latestProgressRef.current = null

				// In broadcast mode, stay in SHARING state instead of transitioning to TRANSPORTING
				if (storeState.isBroadcastMode) {
					setTransferProgress(null)
					setTransferMetadata(null)
					wasManuallyStoppedRef.current = false

					if (progressUpdateIntervalRef.current) {
						clearInterval(progressUpdateIntervalRef.current)
					}
					progressUpdateIntervalRef.current = setInterval(() => {
						const currentViewState = useSenderStore.getState().viewState
						if (latestProgressRef.current && currentViewState === 'SHARING') {
							setTransferProgress(latestProgressRef.current)
						}
					}, 50)
				} else {
					setViewState('TRANSPORTING')
					setTransferProgress(null)
					setTransferMetadata(null)
					wasManuallyStoppedRef.current = false

					if (progressUpdateIntervalRef.current) {
						clearInterval(progressUpdateIntervalRef.current)
					}
					progressUpdateIntervalRef.current = setInterval(() => {
						const currentViewState = useSenderStore.getState().viewState
						if (
							latestProgressRef.current &&
							currentViewState === 'TRANSPORTING'
						) {
							setTransferProgress(latestProgressRef.current)
						}
					}, 50)
				}
			})
			if (disposed) {
				nextUnlistenStart()
			} else {
				unlistenStart = nextUnlistenStart
			}

			const nextUnlistenProgress = await listen(
				'transfer-progress',
				(event: any) => {
					try {
						const storeState = useSenderStore.getState()
						const canAcceptProgress =
							storeState.viewState === 'TRANSPORTING' ||
							(storeState.isBroadcastMode && storeState.viewState === 'SHARING')
						if (!canAcceptProgress) {
							return
						}

						// The engine already windows the speed and derives the ETA
						// from it; averaging again here only adds lag.
						const progress = parseProgressPayload(event.payload as string)
						if (progress) {
							latestProgressRef.current = progress
						}
					} catch (error) {
						console.error('Failed to parse progress event:', error)
					}
				}
			)
			if (disposed) {
				nextUnlistenProgress()
			} else {
				unlistenProgress = nextUnlistenProgress
			}

			const nextUnlistenComplete = await listen(
				'transfer-completed',
				async (event: any) => {
					const storeState = useSenderStore.getState()
					const isTextShare = storeState.sendMode === 'text'
					const generation = shareGenerationRef.current
					const isCurrent = () =>
						!disposed && shareGenerationRef.current === generation

					// Guard: Skip if manually stopped
					if (wasManuallyStoppedRef.current) {
						return
					}

					// Guard: Skip if already reset to IDLE (delayed event after reset)
					if (storeState.viewState === 'IDLE') {
						return
					}

					// Text shares have no source path selected in the file picker.
					if (!storeState.selectedPath && !isTextShare) {
						return
					}

					// Guard: Skip stale completion for a non-active transfer session.
					// In normal mode completion is only valid while actively transporting.
					if (
						!storeState.isBroadcastMode &&
						storeState.viewState !== 'TRANSPORTING'
					) {
						return
					}

					if (progressUpdateIntervalRef.current) {
						clearInterval(progressUpdateIntervalRef.current)
						progressUpdateIntervalRef.current = null
					}

					if (latestProgressRef.current) {
						setTransferProgress(latestProgressRef.current)
					}

					const endTime = Date.now()
					// Prefer the engine's wire time: the wall clock here also
					// covers connection setup and the completion debounce.
					const completion = parseCompletionPayload(event?.payload)
					const duration =
						completion?.durationMs ??
						(transferStartTimeRef.current
							? endTime - transferStartTimeRef.current
							: 0)

					if (isTextShare) {
						const textSize = getTextByteLength(storeState.textDraft)
						setTransferMetadata({
							fileName: t('common:sender.text.clipboardText'),
							fileSize: textSize,
							duration,
							startTime: transferStartTimeRef.current || endTime,
							endTime,
							pathType: 'file',
							itemCount: 1,
						})

						if (storeState.isBroadcastMode) {
							setTimeout(() => {
								if (!isCurrent()) return
								resetForBroadcast()
								latestProgressRef.current = null
								transferStartTimeRef.current = null
							}, 2000)
						} else {
							setViewState('SUCCESS')
							setTransferProgress(null)
						}
						return
					}

					const currentPath = selectedPathRef.current
					const currentPathType = pathTypeRef.current
					const currentBroadcastMode = storeState.isBroadcastMode

					// Use store's selectedPath as source of truth (not the ref)
					const pathToUse = storeState.selectedPath || currentPath
					if (pathToUse) {
						const fileName = pathToUse.split('/').pop() || 'Unknown'
						const estimatedFileSize = latestProgressRef.current?.totalBytes || 0
						const pathTypeToUse = storeState.pathType || currentPathType
						const itemCount = storeState.selectedPaths.length
						const shouldResolveExactSize = itemCount <= 1

						setTransferMetadata({
							fileName,
							fileSize: estimatedFileSize,
							duration,
							startTime: transferStartTimeRef.current || endTime,
							endTime,
							pathType: pathTypeToUse,
							itemCount,
						})

						// Check if broadcast mode is enabled
						if (currentBroadcastMode) {
							// In broadcast mode: reset to listening state after a brief delay
							// Note: active connection count is now managed by active-connection-count event
							setTimeout(() => {
								if (!isCurrent()) return
								resetForBroadcast()
								latestProgressRef.current = null
								transferStartTimeRef.current = null
							}, 2000)
						} else {
							// Normal mode: show success screen
							setViewState('SUCCESS')
							setTransferProgress(null)
						}

						try {
							if (shouldResolveExactSize) {
								const fileSize = await invoke<number>('get_file_size', {
									path: pathToUse,
								})
								if (!isCurrent()) return
								setTransferMetadata({
									fileName,
									fileSize,
									duration,
									startTime: transferStartTimeRef.current || endTime,
									endTime,
									pathType: pathTypeToUse,
									itemCount,
								})
							}
						} catch (error) {
							console.error(
								'[useSender] transfer-completed: failed to get file size:',
								error
							)
						}
					} else {
						// This should never happen now due to guards above, but log if it does
						console.error(
							'[useSender] transfer-completed: no path available to set metadata',
							{
								selectedPathRef: selectedPathRef.current,
								storeSelectedPath: storeState.selectedPath,
								storeViewState: storeState.viewState,
								storeHasMetadata: !!storeState.transferMetadata,
								wasManuallyStopped: wasManuallyStoppedRef.current,
								stackTrace: new Error().stack,
							}
						)
						// Don't set SUCCESS without metadata - just log the error
						// The guards above should prevent this path from being reached
					}
				}
			)
			if (disposed) {
				nextUnlistenComplete()
			} else {
				unlistenComplete = nextUnlistenComplete
			}

			const nextUnlistenFailed = await listen('transfer-failed', async () => {
				const storeState = useSenderStore.getState()
				const isTextShare = storeState.sendMode === 'text'

				// Guard: Skip if manually stopped
				if (wasManuallyStoppedRef.current) {
					return
				}

				// Guard: Skip if already reset to IDLE (delayed event after reset)
				if (storeState.viewState === 'IDLE') {
					return
				}

				// Text shares have no source path selected in the file picker.
				if (!storeState.selectedPath && !isTextShare) {
					return
				}

				// Guard: Skip stale failed event for a non-active transfer session.
				if (
					!storeState.isBroadcastMode &&
					storeState.viewState !== 'TRANSPORTING'
				) {
					return
				}

				if (progressUpdateIntervalRef.current) {
					clearInterval(progressUpdateIntervalRef.current)
					progressUpdateIntervalRef.current = null
				}

				const currentPath = selectedPathRef.current
				const currentPathType = pathTypeRef.current
				const pathToUse = storeState.selectedPath || currentPath
				const pathTypeToUse = storeState.pathType || currentPathType

				// In broadcast mode, reset to SHARING instead of showing SUCCESS
				if (storeState.isBroadcastMode) {
					// Note: active connection count is now managed by active-connection-count event
					resetForBroadcast()
					latestProgressRef.current = null
					transferStartTimeRef.current = null
					return
				}

				const endTime = Date.now()
				const duration = transferStartTimeRef.current
					? endTime - transferStartTimeRef.current
					: 0

				if (isTextShare) {
					setTransferMetadata({
						fileName: t('common:sender.text.clipboardText'),
						fileSize: getTextByteLength(storeState.textDraft),
						duration,
						startTime: transferStartTimeRef.current || endTime,
						endTime,
						wasStopped: true,
						pathType: 'file',
						itemCount: 1,
					})
					setViewState('SUCCESS')
					setTransferProgress(null)
					return
				}

				if (pathToUse) {
					const fileName = pathToUse.split('/').pop() || 'Unknown'
					const itemCount = storeState.selectedPaths.length
					setTransferMetadata({
						fileName,
						fileSize: 0,
						duration,
						startTime: transferStartTimeRef.current || endTime,
						endTime,
						wasStopped: true,
						pathType: pathTypeToUse,
						itemCount,
					})
					setViewState('SUCCESS')
					setTransferProgress(null)
				} else {
					console.warn(
						'[useSender] transfer-failed: NO PATH AVAILABLE - this should not happen due to guards!'
					)
					// Don't set SUCCESS without metadata - guards should prevent this
				}
			})
			if (disposed) {
				nextUnlistenFailed()
			} else {
				unlistenFailed = nextUnlistenFailed
			}

			// Flash Drop can navigate here from a different route. It must wait for
			// these listeners before inviting the selected device, otherwise a very
			// fast acceptance could lose the first progress/completion event.
			if (!disposed) {
				useSenderStore.getState().setTransferEventListenersReady(true)
			}
		}

		setupListeners().catch((error) => {
			console.error('Failed to set up event listeners:', error)
		})

		return () => {
			disposed = true
			useSenderStore.getState().setTransferEventListenersReady(false)
			if (progressUpdateIntervalRef.current) {
				clearInterval(progressUpdateIntervalRef.current)
				progressUpdateIntervalRef.current = null
			}
			safeUnlisten(unlistenStart)
			safeUnlisten(unlistenProgress)
			safeUnlisten(unlistenComplete)
			safeUnlisten(unlistenFailed)
			safeUnlisten(unlistenActiveCount)
			safeUnlisten(unlistenDevicePaired)
			safeUnlisten(unlistenInviteResponse)
			unlistenStart = undefined
			unlistenProgress = undefined
			unlistenComplete = undefined
			unlistenFailed = undefined
			unlistenActiveCount = undefined
			unlistenDevicePaired = undefined
			unlistenInviteResponse = undefined
		}
	}, [
		setViewState,
		setTransferMetadata,
		setTransferProgress,
		resetForBroadcast,
		setActiveConnectionCount,
		refreshPairedDevices,
		setInviteStatus,
		t,
	])

	const handleFilesSelect = async (
		paths: string[],
		providedPathType?: 'file' | 'directory'
	) => {
		if (!paths.length) {
			return
		}

		addSelectedPaths(paths)
		const nextSelectedCount = useSenderStore.getState().selectedPaths.length

		if (nextSelectedCount > 1) {
			setPathType(null)
			return
		}

		if (providedPathType) {
			setPathType(providedPathType)
			return
		}

		if (pathTypeRef.current) {
			return
		}

		try {
			const type = await invoke<string>('check_path_type', { path: paths[0] })
			setPathType(type as 'file' | 'directory')
		} catch (error) {
			console.error('Failed to check path type:', error)
			setPathType(null)
		}
	}

	const handleFileSelect = async (
		path: string,
		providedPathType?: 'file' | 'directory'
	) => {
		await handleFilesSelect([path], providedPathType)
	}

	const clearSelectedPath = () => {
		setSelectedPaths([])
		setPathType(null)
	}

	const removeSelectedPath = (path: string) => {
		removeSelectedPathFromStore(path)
		const remaining = useSenderStore.getState().selectedPaths

		if (remaining.length === 0) {
			setPathType(null)
			return
		}

		if (remaining.length === 1) {
			invoke<string>('check_path_type', { path: remaining[0] })
				.then((type) => {
					setPathType(type as 'file' | 'directory')
				})
				.catch((error) => {
					console.error('Failed to check path type:', error)
					setPathType(null)
				})
			return
		}

		setPathType(null)
	}

	const startSharing = async () => {
		// A second click must not start a second native share before React paints.
		if (useSenderStore.getState().isLoading) return
		if (sendMode === 'file' && !selectedPaths.length) {
			console.warn(
				'[useSender] startSharing: no selectedPaths, returning early'
			)
			return
		}
		const textValidation = validateTextDraft(textDraft)
		if (sendMode === 'text' && !IS_PAIRING_CAPABLE) {
			showAlert(
				t('common:errors.sharingFailed'),
				t('common:sender.text.desktopOnly'),
				'error'
			)
			return
		}
		if (sendMode === 'text' && !textValidation.isValid) {
			showAlert(
				t('common:errors.sharingFailed'),
				textValidation.issue === 'too-large'
					? t('common:sender.text.tooLarge')
					: t('common:sender.text.empty'),
				'error'
			)
			return
		}

		const generation = ++shareGenerationRef.current
		const previous = preparedShareRef.current
		const share: PreparedShare = {
			generation,
			sendMode,
			textDraft,
			selectedPaths: [...selectedPaths],
			preparation: prepareShare({
				previous: Promise.all([
					previous?.preparation.isCancelled()
						? previous.preparation.result
						: previous?.preparation.result.catch(() => {}),
					pendingStopRef.current,
				]),
				prepare: () =>
					sendMode === 'text'
						? invoke<string>('send_text', {
								text: textDraft,
								relay: getRelayConfigArg(),
								discovery: getDiscoveryConfigArg(),
							})
						: invoke<string>('send_items', {
								paths: selectedPaths,
								relay: getRelayConfigArg(),
								discovery: getDiscoveryConfigArg(),
							}),
				dispose: (expectedTicket) => invoke('stop_sharing', { expectedTicket }),
			}),
		}
		preparedShareRef.current = share
		inviteInFlightRef.current = null
		totalSizeCacheRef.current = null
		setPairedInviteStatus({})
		try {
			// The target picker and cancel control are usable while the legacy
			// engine still imports/hashes. No ticket or payload is claimed yet.
			setTicket(null)
			setViewState('SHARING')
			setTransferMetadata(null)
			setTransferProgress(null)
			setActiveConnectionCount(0)
			transferStartTimeRef.current = null
			wasManuallyStoppedRef.current = false
			latestProgressRef.current = null

			setIsLoading(true)
			const result = await share.preparation.result
			if (generation !== shareGenerationRef.current || result === null) return
			setTicket(result)
			setViewState('SHARING')
		} catch (error) {
			if (generation !== shareGenerationRef.current) return
			preparedShareRef.current = null
			inviteInFlightRef.current = null
			setPairedInviteStatus({})
			setViewState('IDLE')
			console.error('[useSender] startSharing: failed:', error)
			showAlert(
				t('common:errors.sharingFailed'),
				isWebPreviewError(error)
					? getWebPreviewErrorMessage(
							error,
							t('common:webPreview.transferUnavailable')
						)
					: `${t('common:errors.sharingFailedDesc')}: ${error}`,
				'error'
			)
		} finally {
			if (generation === shareGenerationRef.current) setIsLoading(false)
		}
	}

	const stopSharing = async () => {
		shareGenerationRef.current += 1
		preparedShareRef.current?.preparation.cancel()
		inviteInFlightRef.current = null
		setPairedInviteStatus({})
		// Always disable broadcast mode when stopping
		setIsBroadcastMode(false)

		try {
			if (useSenderStore.getState().isLoading) {
				wasManuallyStoppedRef.current = true
				const stop = invoke('stop_sharing')
				pendingStopRef.current = stop
				resetToIdle()
				await stop
				return
			}
			const wasActiveTransfer =
				viewState === 'TRANSPORTING' && !transferMetadata?.wasStopped
			const isCompletedTransfer = viewState === 'SUCCESS' && transferMetadata

			const currentSelectedPath = selectedPathRef.current
			const currentTransferStartTime = transferStartTimeRef.current
			const storeState = useSenderStore.getState()
			const isTextShare = storeState.sendMode === 'text'

			if (wasActiveTransfer && (currentSelectedPath || isTextShare)) {
				// In broadcast mode, reset to SHARING instead of showing SUCCESS
				if (storeState.isBroadcastMode) {
					wasManuallyStoppedRef.current = true

					if (progressUpdateIntervalRef.current) {
						clearInterval(progressUpdateIntervalRef.current)
						progressUpdateIntervalRef.current = null
					}

					resetForBroadcast()
					latestProgressRef.current = null
					transferStartTimeRef.current = null
				} else {
					wasManuallyStoppedRef.current = true

					if (progressUpdateIntervalRef.current) {
						clearInterval(progressUpdateIntervalRef.current)
						progressUpdateIntervalRef.current = null
					}

					const endTime = Date.now()
					const fileName = isTextShare
						? t('common:sender.text.clipboardText')
						: currentSelectedPath?.split('/').pop() || 'Unknown'
					const currentPathType = isTextShare ? 'file' : pathTypeRef.current
					const itemCount = isTextShare ? 1 : storeState.selectedPaths.length

					const stoppedMetadata: TransferMetadata = {
						fileName,
						fileSize: isTextShare ? getTextByteLength(storeState.textDraft) : 0,
						duration: 0,
						startTime: currentTransferStartTime || endTime,
						endTime,
						wasStopped: true,
						pathType: currentPathType,
						itemCount,
					}

					setTransferMetadata(stoppedMetadata)
					setViewState('SUCCESS')
					latestProgressRef.current = null
					transferStartTimeRef.current = null
				}
			}

			if (isCompletedTransfer) {
				wasManuallyStoppedRef.current = false
				resetToIdle()
				transferStartTimeRef.current = null
				latestProgressRef.current = null

				invoke('stop_sharing').catch((error) => {
					console.warn('Background cleanup failed (non-critical):', error)
				})
				return
			}

			await invoke('stop_sharing')

			// If no active transfer (just sharing, waiting for acceptance), reset to idle
			if (!wasActiveTransfer || (!currentSelectedPath && !isTextShare)) {
				wasManuallyStoppedRef.current = false
				setActiveConnectionCount(0)
				resetToIdle()
				transferStartTimeRef.current = null
				latestProgressRef.current = null
				return
			}

			setTicket(null)
			setSelectedPaths([])
			setPathType(null)
			setTransferProgress(null)
			transferStartTimeRef.current = null
			latestProgressRef.current = null
		} catch (error) {
			console.error('Failed to stop sharing:', error)
			showAlert(
				t('common:errors.stopSharingFailed'),
				`${t('common:errors.stopSharingFailedDesc')}: ${error}`,
				'error'
			)
		}
	}

	const resetForNewTransfer = async () => {
		await stopSharing()
	}

	const resolveShareTotalSize = (
		share: PreparedShare | null
	): Promise<number> => {
		const state = useSenderStore.getState()
		const mode = share?.sendMode ?? state.sendMode
		if (mode === 'text') {
			return Promise.resolve(
				getTextByteLength(share?.textDraft ?? state.textDraft)
			)
		}
		if (state.transferMetadata?.fileSize != null) {
			return Promise.resolve(state.transferMetadata.fileSize)
		}
		if (share?.totalSize) return share.totalSize
		const cached = totalSizeCacheRef.current
		if (!share && state.ticket && cached?.ticket === state.ticket) {
			return cached.size
		}
		const size = Promise.all(
			(share?.selectedPaths ?? state.selectedPaths).map((path) =>
				invoke<number>('get_file_size', { path }).catch(() => 0)
			)
		).then((sizes) => sizes.reduce((sum, value) => sum + value, 0))
		if (share) share.totalSize = size
		else if (state.ticket) {
			totalSizeCacheRef.current = { ticket: state.ticket, size }
		}
		return size
	}

	const getInviteContext = () => {
		const generation = shareGenerationRef.current
		const pending = preparedShareRef.current
		const share =
			pending?.generation === generation && !pending.preparation.isCancelled()
				? pending
				: null
		const state = useSenderStore.getState()
		if (!share && !state.ticket) return null
		return {
			share,
			ticket: state.ticket,
			generation,
			fileCount:
				(share?.sendMode ?? state.sendMode) === 'text'
					? 1
					: Math.max((share?.selectedPaths ?? state.selectedPaths).length, 1),
			isCurrent: () =>
				shareGenerationRef.current === generation &&
				!share?.preparation.isCancelled() &&
				useSenderStore.getState().viewState !== 'IDLE',
		}
	}

	const onInvitePairedDevice = async (endpointId: string): Promise<boolean> => {
		const context = getInviteContext()
		if (!context) return false
		if (!isNodeReady) {
			toastManager.add({
				title: t('common:settings.devices.nodeUnavailableTitle'),
				description: t('common:settings.devices.nodeUnavailableHint'),
				type: 'error',
			})
			return false
		}
		if (inviteInFlightRef.current) return false
		const anotherInviteInFlight = Object.entries(pairedInviteStatus).some(
			([id, status]) =>
				id !== endpointId &&
				(status === 'preparing' || status === 'sending' || status === 'sent')
		)
		if (anotherInviteInFlight || pairedInviteStatus[endpointId] === 'sent') {
			return false
		}

		const device =
			pairedDevices.find((d) => d.endpoint_id === endpointId) ?? null
		const deviceName =
			device?.display_name ?? t('common:sender.pairedDevices.unknownPeer')
		if (device && (!isPairedDeviceActive(device) || !device.online)) {
			return false
		}
		incrementPairedSendCount(endpointId)
		inviteInFlightRef.current = endpointId
		setInviteStatus(endpointId, context.ticket ? 'sending' : 'preparing')
		try {
			// The user's selection and the size lookup can overlap legacy
			// import/hash work. Only a finished ticket may reach the peer.
			const [readyTicket, totalSize] = await Promise.all([
				context.share?.preparation.result ?? Promise.resolve(context.ticket),
				resolveShareTotalSize(context.share),
			])
			if (!context.isCurrent() || !readyTicket) return false
			setInviteStatus(endpointId, 'sending')
			const delivered = await invitePairedDevice(
				endpointId,
				readyTicket,
				context.fileCount,
				totalSize
			)
			if (!context.isCurrent()) return false
			if (delivered) {
				setInviteStatus(endpointId, 'sent')
				toastManager.add({
					title: t('common:sender.pairedDevices.inviteSentTo', {
						name: deviceName,
					}),
					description: t('common:sender.pairedDevices.inviteSentDesc'),
					type: 'success',
				})
				return true
			}
			setInviteStatus(endpointId, 'failed')
			toastManager.add({
				title: t('common:sender.pairedDevices.inviteFailed'),
				description: t('common:sender.pairedDevices.deviceUnreachable'),
				type: 'error',
			})
			setTimeout(() => {
				if (context.isCurrent()) setInviteStatus(endpointId, null)
			}, 4000)
			inviteInFlightRef.current = null
			return false
		} catch (error) {
			if (!context.isCurrent()) return false
			inviteInFlightRef.current = null
			setInviteStatus(endpointId, 'failed')
			toastManager.add({
				title: t('common:sender.pairedDevices.inviteFailed'),
				description: String(error),
				type: 'error',
			})
			setTimeout(() => {
				if (context.isCurrent()) setInviteStatus(endpointId, null)
			}, 4000)
			return false
		}
	}

	const onInviteNearbyDevice = async (endpointId: string): Promise<boolean> => {
		const context = getInviteContext()
		if (!context) return false
		if (!isNodeReady) {
			toastManager.add({
				title: t('common:settings.devices.nodeUnavailableTitle'),
				description: t('common:settings.devices.nodeUnavailableHint'),
				type: 'error',
			})
			return false
		}
		if (inviteInFlightRef.current) return false
		const anotherInviteInFlight = Object.entries(pairedInviteStatus).some(
			([id, status]) =>
				id !== endpointId &&
				(status === 'preparing' || status === 'sending' || status === 'sent')
		)
		if (anotherInviteInFlight || pairedInviteStatus[endpointId] === 'sent') {
			return false
		}

		const nearbyDevices = useNearbyStore.getState().devices
		const nearby = nearbyDevices.find((d) => d.endpointId === endpointId)
		const nearbyName =
			nearby?.displayName ??
			(endpointId
				? `${endpointId.slice(0, 8)}…`
				: t('common:sender.pairedDevices.unknownPeer'))
		inviteInFlightRef.current = endpointId
		setInviteStatus(endpointId, context.ticket ? 'sending' : 'preparing')
		try {
			const [readyTicket, totalSize] = await Promise.all([
				context.share?.preparation.result ?? Promise.resolve(context.ticket),
				resolveShareTotalSize(context.share),
			])
			if (!context.isCurrent() || !readyTicket) return false
			setInviteStatus(endpointId, 'sending')
			const delivered = await inviteNearbyDevice(
				endpointId,
				readyTicket,
				context.fileCount,
				totalSize
			)
			if (!context.isCurrent()) return false
			if (delivered) {
				setInviteStatus(endpointId, 'sent')
				toastManager.add({
					title: t('common:sender.pairedDevices.inviteSentTo', {
						name: nearbyName,
					}),
					description: t('common:sender.pairedDevices.inviteSentDesc'),
					type: 'success',
				})
				// The receiver is being asked to check this code against our
				// screen. Only on confirmed delivery — a code for an invite that
				// never arrived is worse than none.
				useNearbyVerificationStore
					.getState()
					.show({ endpointId, name: nearbyName })
				return true
			}
			setInviteStatus(endpointId, 'failed')
			toastManager.add({
				title: t('common:sender.pairedDevices.inviteFailed'),
				description: t('common:settings.devices.nearby.inviteFailed'),
				type: 'error',
			})
			setTimeout(() => {
				if (context.isCurrent()) setInviteStatus(endpointId, null)
			}, 4000)
			inviteInFlightRef.current = null
			return false
		} catch (error) {
			if (!context.isCurrent()) return false
			inviteInFlightRef.current = null
			setInviteStatus(endpointId, 'failed')
			toastManager.add({
				title: t('common:sender.pairedDevices.inviteFailed'),
				description: String(error),
				type: 'error',
			})
			setTimeout(() => {
				if (context.isCurrent()) setInviteStatus(endpointId, null)
			}, 4000)
			return false
		}
	}

	const copyTicket = async () => {
		if (ticket) {
			try {
				await copyTextToClipboard(ticket)
				setCopySuccess(true)
				setTimeout(() => setCopySuccess(false), 2000)
			} catch (error) {
				console.error('Failed to copy ticket:', error)
				showAlert(
					t('common:errors.copyFailed'),
					`${t('common:errors.copyFailedDesc')}: ${error}`,
					'error'
				)
			}
		}
	}

	// Derived states for backward compatibility
	const isSharing = viewState === 'SHARING' || viewState === 'TRANSPORTING'
	const isTransporting = viewState === 'TRANSPORTING'
	const isCompleted = viewState === 'SUCCESS'

	return {
		viewState,
		isSharing,
		isTransporting,
		isCompleted,
		ticket,
		selectedPaths,
		selectedPath,
		pathType,
		isLoading,
		copySuccess,
		alertDialog,
		transferMetadata,
		transferProgress,
		sendMode,
		textDraft,
		isBroadcastMode,
		activeConnectionCount,
		pairedDevices,
		isNodeReady,
		isNodeStatusPending,
		pairedInviteStatus,
		onInvitePairedDevice,
		onInviteNearbyDevice,

		handleFileSelect,
		handleFilesSelect,
		clearSelectedPath,
		removeSelectedPath,
		setSendMode,
		setTextDraft,
		clearTextDraft,
		startSharing,
		stopSharing,
		copyTicket,
		showAlert,
		closeAlert,
		resetForNewTransfer,
	}
}
