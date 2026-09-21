import { useEffect } from 'react'
import { Outlet, useNavigate } from 'react-router-dom'
import { invoke, listen, openDialog } from '@/lib/platform-api'
import { useSenderStore } from '@/store/sender-store'
import { useTransferTabStore } from '@/store/transfer-tab-store'
import { toastManager } from '../ui/toast'

import { AppFooter } from '../AppFooter'
import { FlashDropBridge } from '@/components/flash-drop'
import { AndroidTextShareBridge } from '@/components/sender/AndroidTextShareBridge'
import { TitleBar } from '../TitleBar'
import { useTranslation } from '@/i18n'
import { AppUpdater } from '../common/AppUpdater'
import { DeviceNodeSync } from '../pairing/DeviceNodeSync'
import { NearbyInviteDialog } from '../pairing/NearbyInviteDialog'
import { NearbyPairRequestDialog } from '../pairing/NearbyPairRequestDialog'
import { NearbyVerificationDialog } from '../pairing/NearbyVerificationDialog'
import { PairedInviteDialog } from '../pairing/PairedInviteDialog'
import { ReceiverProvider } from '../receiver/ReceiverProvider'
import { WindowsContextMenuSync } from '../settings/system-tray/context-menu-toggle'
import { useIsWindowsPortable } from '@/hooks/use-windows-portable'
import { useAutostartFirstRun } from '../../hooks/useAutostartFirstRun'
import { useTrayLabels } from '../../hooks/useTrayLabels'
import {
	IS_ANDROID,
	IS_LINUX,
	IS_MACOS,
	IS_PAIRING_CAPABLE,
	IS_UPDATER_AVAILABLE,
	IS_WEB,
	IS_WINDOWS,
} from '@/lib/platform'

export function RootLayout() {
	const navigate = useNavigate()
	useEffect(() => {
		const choose = async (directory: boolean) => {
			const busy = () => {
				const state = useSenderStore.getState()
				return (
					state.isLoading ||
					state.viewState === 'SHARING' ||
					state.viewState === 'TRANSPORTING'
				)
			}
			if (busy()) {
				toastManager.add({
					title: '请先完成当前分享，再添加文件',
					type: 'warning',
				})
				return
			}
			const selected = await openDialog({ multiple: true, directory })
			if (!selected || busy()) return
			const paths = Array.isArray(selected) ? selected : [selected]
			if (!paths.length) return
			const sender = useSenderStore.getState()
			sender.setSendMode('file')
			sender.addSelectedPaths(paths)
			const path = useSenderStore.getState().selectedPaths[0]
			sender.setPathType(
				await invoke<'file' | 'directory'>('check_path_type', { path })
			)
			useTransferTabStore.getState().requestTab('send')
			navigate('/')
		}
		const report = (error: unknown) =>
			toastManager.add({
				title: '无法选择文件',
				description: String(error),
				type: 'error',
			})
		const stops = [
			listen('launch-intent', () => navigate('/')),
			listen('open-settings', () => navigate('/settings')),
			listen('choose-share-files', () => {
				void choose(false).catch(report)
			}),
			listen('choose-share-folder', () => {
				void choose(true).catch(report)
			}),
		]
		return () => {
			for (const stop of stops) void stop.then((unlisten) => unlisten())
		}
	}, [navigate])
	const { t } = useTranslation('common')
	const { data: isWindowsPortable = false } = useIsWindowsPortable()
	useTrayLabels()
	useAutostartFirstRun()
	return (
		<ReceiverProvider>
			<FlashDropBridge />
			<AndroidTextShareBridge />
			{/* Mounts the periodic check as well as the banner, so this gate decides
			    whether the app checks for updates at all — Android included. */}
			{IS_UPDATER_AVAILABLE && !isWindowsPortable && <AppUpdater />}
			{IS_WINDOWS && <WindowsContextMenuSync />}
			{IS_PAIRING_CAPABLE && <DeviceNodeSync />}
			{IS_PAIRING_CAPABLE && <PairedInviteDialog />}
			{IS_PAIRING_CAPABLE && <NearbyInviteDialog />}
			{IS_PAIRING_CAPABLE && <NearbyPairRequestDialog />}
			{IS_PAIRING_CAPABLE && <NearbyVerificationDialog />}
			<main
				className={
					IS_WEB
						? 'h-full flex flex-col relative glass-background select-none bg-background'
						: 'h-dvh min-h-screen flex flex-col relative glass-background select-none bg-background'
				}
			>
				{IS_LINUX && !IS_ANDROID && <TitleBar title={t('appTitle')} />}

				{IS_MACOS && (
					<div className="absolute w-full h-10 z-10" data-tauri-drag-region />
				)}
				<Outlet />
				<AppFooter />
			</main>
		</ReceiverProvider>
	)
}
