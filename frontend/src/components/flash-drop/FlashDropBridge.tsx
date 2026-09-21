import { useCallback, useEffect, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { toastManager } from '@/components/ui/toast'
import { useTranslation } from '@/i18n'
import { getDiscoveryConfigArg } from '@/lib/discovery'
import { incrementPairedSendCount } from '@/lib/paired-send-counts'
import {
	getNodeStatus,
	invitePairedDevice,
	isPairedDeviceActive,
	listPairedDevices,
	type PairedDevice,
} from '@/lib/pairing-api'
import { IS_MACOS } from '@/lib/platform'
import { invoke, listen, type UnlistenFn } from '@/lib/platform-api'
import { getRelayConfigArg } from '@/lib/relay'
import { useAppSettingStore } from '@/store/app-setting'
import { useSenderStore } from '@/store/sender-store'
import { useTransferTabStore } from '@/store/transfer-tab-store'

type FlashDropFilesEvent = { payload: unknown }
type FlashDropToggleEvent = { payload: unknown }

const SENDER_LISTENER_TIMEOUT_MS = 4_000

function isSenderBusy() {
	const sender = useSenderStore.getState()
	return (
		sender.textDraft.length > 0 ||
		sender.isLoading ||
		sender.viewState === 'SHARING' ||
		sender.viewState === 'TRANSPORTING'
	)
}

function hasUnsentSelection() {
	return useSenderStore.getState().selectedPaths.length > 0
}

function pathsFromPayload(payload: unknown): string[] {
	if (!Array.isArray(payload)) return []
	return Array.from(
		new Set(
			payload.filter(
				(path): path is string => typeof path === 'string' && path.length > 0
			)
		)
	)
}

async function selectedOnlinePairedDevice(
	targetId: string
): Promise<PairedDevice> {
	const nodeStatus = await getNodeStatus()
	if (nodeStatus.status !== 'ready') {
		throw new Error('pairing_node_unavailable')
	}

	const device = (await listPairedDevices()).find(
		(candidate) => candidate.endpoint_id === targetId
	)
	if (!device || !isPairedDeviceActive(device)) {
		throw new Error('target_not_paired')
	}
	if (!device.online) {
		throw new Error('target_offline')
	}
	return device
}

async function selectedStoredDevice(
	targetId: string
): Promise<PairedDevice | null> {
	const devices = await listPairedDevices()
	return (
		devices.find(
			(device) =>
				device.endpoint_id === targetId && isPairedDeviceActive(device)
		) ?? null
	)
}

function waitForSenderEventListeners(): Promise<void> {
	if (useSenderStore.getState().transferEventListenersReady) {
		return Promise.resolve()
	}

	return new Promise((resolve, reject) => {
		let timeout: number | undefined
		const stop = useSenderStore.subscribe((state, previous) => {
			if (
				!previous.transferEventListenersReady &&
				state.transferEventListenersReady
			) {
				if (timeout !== undefined) {
					window.clearTimeout(timeout)
				}
				stop()
				resolve()
			}
		})
		timeout = window.setTimeout(() => {
			stop()
			reject(new Error('sender_listeners_not_ready'))
		}, SENDER_LISTENER_TIMEOUT_MS)
	})
}

async function totalSize(paths: string[]): Promise<number> {
	const sizes = await Promise.all(
		paths.map((path) =>
			invoke<number>('get_file_size', { path }).catch(() => 0)
		)
	)
	return sizes.reduce((sum, size) => sum + size, 0)
}

/**
 * Owns the macOS-only native drop surface. It deliberately uses the sender
 * store and the same Tauri commands as useSender instead of mounting a second
 * useSender instance, which would duplicate transfer lifecycle subscriptions.
 */
export function FlashDropBridge() {
	const navigate = useNavigate()
	const { t } = useTranslation()
	const enabled = useAppSettingStore((state) => state.flashDropEnabled)
	const targetId = useAppSettingStore((state) => state.flashDropTargetId)
	const setEnabled = useAppSettingStore((state) => state.setFlashDropEnabled)
	const setTargetId = useAppSettingStore((state) => state.setFlashDropTargetId)
	const syncGeneration = useRef(0)
	const sendInFlight = useRef(false)
	const flashTransferActive = useRef(false)

	const report = useCallback(async (message: string, success: boolean) => {
		try {
			await invoke('flash_drop_feedback', { message, success })
		} catch (error) {
			console.warn('Failed to show Flash Drop feedback:', error)
		}
	}, [])

	const openSettings = useCallback(() => {
		void invoke('focus_main_window').catch(() => {})
		navigate('/settings')
	}, [navigate])

	const syncNativeSurface = useCallback(async () => {
		const generation = ++syncGeneration.current
		if (!enabled || !targetId) {
			await invoke('configure_flash_drop', {
				enabled: false,
				targetLabel: '',
				available: false,
			})
			return
		}

		try {
			const target = await selectedStoredDevice(targetId)
			if (generation !== syncGeneration.current) return

			if (!target) {
				// An unpaired or stale identity cannot remain an implicit target.
				setTargetId(null)
				setEnabled(false)
				await invoke('configure_flash_drop', {
					enabled: false,
					targetLabel: '',
					available: false,
				})
				return
			}

			await invoke('configure_flash_drop', {
				enabled: true,
				targetLabel: target.display_name,
				available: target.online,
			})
		} catch (error) {
			if (generation !== syncGeneration.current) return
			// A node restart is transient. Keep the selected target persisted, but
			// leave the native surface unavailable until the next presence refresh.
			console.warn('Failed to refresh Flash Drop target:', error)
			await invoke('configure_flash_drop', {
				enabled: false,
				targetLabel: '',
				available: false,
			}).catch(() => {})
		}
	}, [enabled, setEnabled, setTargetId, targetId])

	const enableFromTray = useCallback(
		async (requested: boolean) => {
			if (!requested) {
				setEnabled(false)
				return
			}

			const currentTargetId = useAppSettingStore.getState().flashDropTargetId
			if (!currentTargetId) {
				setEnabled(false)
				await report(t('settings.general.flashDrop.targetRequired'), false)
				openSettings()
				return
			}

			try {
				await selectedOnlinePairedDevice(currentTargetId)
				setEnabled(true)
			} catch {
				setEnabled(false)
				await report(t('settings.general.flashDrop.targetUnavailable'), false)
				openSettings()
			}
		},
		[openSettings, report, setEnabled, t]
	)

	const handleFiles = useCallback(
		async (paths: string[]) => {
			if (sendInFlight.current || isSenderBusy()) {
				await report(t('settings.general.flashDrop.busy'), false)
				return
			}
			if (hasUnsentSelection()) {
				await report(t('settings.general.flashDrop.selectionPending'), false)
				return
			}

			const settings = useAppSettingStore.getState()
			if (!settings.flashDropEnabled || !settings.flashDropTargetId) {
				await report(t('settings.general.flashDrop.targetRequired'), false)
				openSettings()
				return
			}

			sendInFlight.current = true
			let selectionChanged = false
			let shareStarted = false
			try {
				const target = await selectedOnlinePairedDevice(
					settings.flashDropTargetId
				)
				await invoke('focus_main_window')
				useTransferTabStore.getState().requestTab('send')
				navigate('/')
				await waitForSenderEventListeners()

				if (isSenderBusy() || hasUnsentSelection()) {
					throw new Error('sender_busy')
				}
				const latestSettings = useAppSettingStore.getState()
				if (
					!latestSettings.flashDropEnabled ||
					latestSettings.flashDropTargetId !== settings.flashDropTargetId
				) {
					throw new Error('flash_drop_settings_changed')
				}

				const sender = useSenderStore.getState()
				sender.resetToIdle()
				sender.setSendMode('file')
				sender.setSelectedPaths(paths)
				selectionChanged = true
				if (paths.length === 1) {
					const pathType = await invoke<'file' | 'directory'>(
						'check_path_type',
						{
							path: paths[0],
						}
					)
					sender.setPathType(pathType)
				} else {
					sender.setPathType(null)
				}

				await report(
					t('settings.general.flashDrop.preparing', {
						name: target.display_name,
					}),
					true
				)
				sender.setIsLoading(true)
				const ticket = await invoke<string>('send_items', {
					paths,
					relay: getRelayConfigArg(),
					discovery: getDiscoveryConfigArg(),
				})
				shareStarted = true
				sender.setTicket(ticket)
				sender.setViewState('SHARING')
				sender.setIsLoading(false)

				// Refresh immediately before the invitation so a just-unpaired device
				// cannot receive a ticket from a stale presence snapshot.
				const targetIdBeforeInvite =
					useAppSettingStore.getState().flashDropTargetId
				if (
					!useAppSettingStore.getState().flashDropEnabled ||
					targetIdBeforeInvite !== settings.flashDropTargetId
				) {
					throw new Error('flash_drop_settings_changed')
				}
				const currentTarget =
					await selectedOnlinePairedDevice(targetIdBeforeInvite)
				flashTransferActive.current = true
				incrementPairedSendCount(currentTarget.endpoint_id)
				const delivered = await invitePairedDevice(
					currentTarget.endpoint_id,
					ticket,
					Math.max(paths.length, 1),
					await totalSize(paths)
				)
				if (!delivered) {
					throw new Error('invite_not_delivered')
				}

				toastManager.add({
					title: t('settings.general.flashDrop.waiting', {
						name: currentTarget.display_name,
					}),
					type: 'success',
				})
				await report(
					t('settings.general.flashDrop.waiting', {
						name: currentTarget.display_name,
					}),
					true
				)
			} catch (error) {
				flashTransferActive.current = false
				if (selectionChanged || shareStarted) {
					const sender = useSenderStore.getState()
					sender.setIsLoading(false)
					if (shareStarted) {
						await invoke('stop_sharing').catch(() => {})
					}
					sender.resetToIdle()
				}

				const message =
					error instanceof Error && error.message === 'sender_busy'
						? t('settings.general.flashDrop.busy')
						: error instanceof Error &&
								error.message === 'flash_drop_settings_changed'
							? t('settings.general.flashDrop.settingsChanged')
							: error instanceof Error &&
									error.message === 'sender_listeners_not_ready'
								? t('settings.general.flashDrop.senderNotReady')
								: error instanceof Error &&
										(error.message === 'target_not_paired' ||
											error.message === 'target_offline')
									? t('settings.general.flashDrop.targetUnavailable')
									: error instanceof Error &&
											error.message === 'pairing_node_unavailable'
										? t('settings.general.flashDrop.nodeUnavailable')
										: t('settings.general.flashDrop.failed')
				console.error('Flash Drop failed:', error)
				toastManager.add({ title: message, type: 'error' })
				await report(message, false)
			} finally {
				sendInFlight.current = false
			}
		},
		[navigate, openSettings, report, t]
	)

	useEffect(() => {
		if (!IS_MACOS) return
		void syncNativeSurface()
	}, [syncNativeSurface])

	useEffect(() => {
		if (!IS_MACOS) return
		return useSenderStore.subscribe((state, previous) => {
			// A user can stop the waiting share from the regular sender UI. Native
			// sender events are direction-specific (`transfer-*` vs `receive-*`),
			// and returning to IDLE means this Flash Drop send is no longer live.
			if (
				flashTransferActive.current &&
				previous.viewState !== 'IDLE' &&
				state.viewState === 'IDLE'
			) {
				flashTransferActive.current = false
			}
		})
	}, [])

	useEffect(() => {
		if (!IS_MACOS) return
		let disposed = false
		const setup = async () => {
			const eventNames = [
				'paired-device-presence',
				'device-paired',
				'device-unpaired',
				'identity-rotated',
			]
			const stops = await Promise.all(
				eventNames.map((eventName) =>
					listen(eventName, () => void syncNativeSurface())
				)
			)
			if (disposed) {
				stops.forEach((stop) => stop())
				return
			}
			unlisten = stops
		}
		let unlisten: UnlistenFn[] = []
		void setup().catch((error) => {
			console.warn('Failed to subscribe to Flash Drop target events:', error)
		})
		return () => {
			disposed = true
			unlisten.forEach((stop) => stop())
		}
	}, [syncNativeSurface])

	useEffect(() => {
		if (!IS_MACOS) return
		let disposed = false
		let unlistenFiles: UnlistenFn | undefined
		let unlistenToggle: UnlistenFn | undefined
		let unlistenComplete: UnlistenFn | undefined
		let unlistenFailed: UnlistenFn | undefined

		const setup = async () => {
			const files = await listen(
				'flash-drop-files',
				(event: FlashDropFilesEvent) => {
					const paths = pathsFromPayload(event.payload)
					if (!paths.length) {
						void report(t('settings.general.flashDrop.invalidFiles'), false)
						return
					}
					void handleFiles(paths)
				}
			)
			const toggle = await listen(
				'toggle-flash-drop',
				(event: FlashDropToggleEvent) => {
					void enableFromTray(event.payload === true)
				}
			)
			const complete = await listen('transfer-completed', () => {
				if (!flashTransferActive.current) return
				flashTransferActive.current = false
				void report(t('settings.general.flashDrop.completed'), true)
			})
			const failed = await listen('transfer-failed', () => {
				if (!flashTransferActive.current) return
				flashTransferActive.current = false
				void report(t('settings.general.flashDrop.transferFailed'), false)
			})

			if (disposed) {
				files()
				toggle()
				complete()
				failed()
				return
			}
			unlistenFiles = files
			unlistenToggle = toggle
			unlistenComplete = complete
			unlistenFailed = failed
		}

		void setup().catch((error) => {
			console.warn('Failed to subscribe to Flash Drop events:', error)
		})
		return () => {
			disposed = true
			unlistenFiles?.()
			unlistenToggle?.()
			unlistenComplete?.()
			unlistenFailed?.()
		}
	}, [enableFromTray, handleFiles, report, t])

	return null
}
