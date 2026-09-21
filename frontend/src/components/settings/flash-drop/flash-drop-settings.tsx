import { Loader2 } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { useTranslation } from '@/i18n'
import {
	isPairedDeviceActive,
	listPairedDevices,
	type PairedDevice,
} from '@/lib/pairing-api'
import { listen } from '@/lib/platform-api'
import { useAppSettingStore } from '@/store/app-setting'
import {
	Frame,
	FrameDescription,
	FrameHeader,
	FramePanel,
	FrameTitle,
} from '../../ui/frame'
import {
	Select,
	SelectItem,
	SelectPopup,
	SelectTrigger,
	SelectValue,
} from '../../ui/select'
import { Switch } from '../../ui/switch'
import { toastManager } from '../../ui/toast'

export function FlashDropSettings() {
	const { t } = useTranslation()
	const enabled = useAppSettingStore((state) => state.flashDropEnabled)
	const targetId = useAppSettingStore((state) => state.flashDropTargetId)
	const setEnabled = useAppSettingStore((state) => state.setFlashDropEnabled)
	const setTargetId = useAppSettingStore((state) => state.setFlashDropTargetId)
	const [devices, setDevices] = useState<PairedDevice[]>([])
	const [isLoading, setIsLoading] = useState(true)

	const refreshDevices = useCallback(async () => {
		try {
			setDevices(await listPairedDevices())
		} catch (error) {
			console.warn('Failed to load Flash Drop targets:', error)
			setDevices([])
		} finally {
			setIsLoading(false)
		}
	}, [])

	useEffect(() => {
		void refreshDevices()
	}, [refreshDevices])

	useEffect(() => {
		let disposed = false
		let stops: (() => void)[] = []
		const setup = async () => {
			stops = await Promise.all(
				['paired-device-presence', 'device-paired', 'device-unpaired'].map(
					(eventName) => listen(eventName, () => void refreshDevices())
				)
			)
			if (disposed) stops.forEach((stop) => stop())
		}
		void setup().catch((error) => {
			console.warn('Failed to watch Flash Drop targets:', error)
		})
		return () => {
			disposed = true
			stops.forEach((stop) => stop())
		}
	}, [refreshDevices])

	const pairedDevices = useMemo(
		() => devices.filter(isPairedDeviceActive),
		[devices]
	)
	const selectedTarget = pairedDevices.find(
		(device) => device.endpoint_id === targetId
	)
	const canEnable = Boolean(selectedTarget?.online)

	const handleEnabledChange = (next: boolean) => {
		if (next && !canEnable) {
			toastManager.add({
				title: t('settings.general.flashDrop.targetRequired'),
				description: t('settings.general.flashDrop.targetRequiredHint'),
				type: 'warning',
			})
			return
		}
		setEnabled(next)
	}

	return (
		<Frame>
			<FrameHeader>
				<FrameTitle>{t('settings.general.flashDrop.title')}</FrameTitle>
			</FrameHeader>
			<FramePanel className="space-y-4">
				<div className="flex items-center justify-between gap-4">
					<div className="min-w-0 flex-1">
						<FrameTitle>{t('settings.general.flashDrop.enabled')}</FrameTitle>
						<FrameDescription>
							{t('settings.general.flashDrop.enabledHint')}
						</FrameDescription>
					</div>
					<Switch checked={enabled} onCheckedChange={handleEnabledChange} />
				</div>

				<div className="space-y-2 border-t pt-4">
					<FrameTitle>{t('settings.general.flashDrop.target')}</FrameTitle>
					<Select value={targetId ?? undefined}>
						<SelectTrigger className="w-full">
							<SelectValue
								placeholder={t('settings.general.flashDrop.targetPlaceholder')}
							>
								{(value: string | null) => {
									const target = pairedDevices.find(
										(device) => device.endpoint_id === value
									)
									if (!target) return null
									return target.online
										? target.display_name
										: `${target.display_name} (${t(
												'settings.general.flashDrop.offline'
											)})`
								}}
							</SelectValue>
						</SelectTrigger>
						<SelectPopup>
							{pairedDevices.map((device) => (
								<SelectItem
									key={device.endpoint_id}
									value={device.endpoint_id}
									disabled={!device.online}
									onClick={() => setTargetId(device.endpoint_id)}
								>
									{device.online
										? device.display_name
										: `${device.display_name} (${t(
												'settings.general.flashDrop.offline'
											)})`}
								</SelectItem>
							))}
						</SelectPopup>
					</Select>
					{isLoading ? (
						<p className="flex items-center gap-2 text-sm text-muted-foreground">
							<Loader2 className="size-4 animate-spin" />
							{t('loading')}
						</p>
					) : pairedDevices.length === 0 ? (
						<FrameDescription>
							{t('settings.general.flashDrop.noPairedDevices')}{' '}
							<Link
								className="underline underline-offset-2 hover:text-foreground"
								to="/settings/devices"
							>
								{t('settings.general.flashDrop.manageDevices')}
							</Link>
						</FrameDescription>
					) : !canEnable ? (
						<FrameDescription>
							{t('settings.general.flashDrop.targetRequiredHint')}
						</FrameDescription>
					) : null}
				</div>
			</FramePanel>
		</Frame>
	)
}
