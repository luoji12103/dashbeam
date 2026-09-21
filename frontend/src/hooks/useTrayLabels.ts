import { useEffect } from 'react'
import { IS_DESKTOP } from '@/lib/platform'
import { invoke } from '@/lib/platform-api'
import { useTranslation } from '@/i18n'

/**
 * Keeps the native tray menu in the app's language — it is built in Rust with
 * English defaults before the webview exists.
 */
export function useTrayLabels(): void {
	const { t, i18n } = useTranslation()

	// t's identity is stable across language changes in this repo's i18n shim
	// (it's always i18next.t), so i18n.language must stay in the deps array to
	// re-run the effect when the language actually switches.
	// biome-ignore lint/correctness/useExhaustiveDependencies: see comment above
	useEffect(() => {
		if (!IS_DESKTOP) return
		void invoke('set_tray_labels', {
			labels: {
				settings: i18n.language.startsWith('zh') ? '快捷设置…' : 'Settings…',
				share_files: i18n.language.startsWith('zh')
					? '选择文件分享…'
					: 'Share Files…',
				share_folder: i18n.language.startsWith('zh')
					? '选择文件夹分享…'
					: 'Share Folder…',
				flash_drop: i18n.language.startsWith('zh')
					? '右上角闪传'
					: 'Flash Drop',
				target_none: i18n.language.startsWith('zh')
					? '闪传目标：未选择'
					: 'Target: Not selected',
				target_selected: i18n.language.startsWith('zh')
					? '闪传目标：{{name}}'
					: 'Target: {{name}}',
				open: t('settings.general.systembar.trayOpen'),
				quit: t('settings.general.systembar.trayQuit'),
				no_devices: t('settings.general.systembar.trayNoDevices'),
				devices_online: t('settings.general.systembar.trayDevicesOnline'),
				device_online: t('settings.general.systembar.trayDeviceOnline'),
			},
		}).catch(() => {
			// Tray may have failed to build; English defaults stay.
		})
	}, [t, i18n.language])
}
