import {
	IS_DESKTOP,
	IS_MACOS,
	IS_TAURI,
	IS_UPDATER_AVAILABLE,
} from '@/lib/platform'
import MobileSettingSidebar from '../components/setting-sidebar/mobile-setting-sidebar'
import { AutoUpdate } from '../components/settings/auto-update'
import { BroadcastSettings } from '../components/settings/broadcast'
import { DebugMode } from '../components/settings/debug-mode'
import { FlashDropSettings } from '../components/settings/flash-drop'
import { Notifications } from '../components/settings/notifications'
import { RelayStatusSettings } from '../components/settings/relay-status'
import { ReceivedTextSettings } from '../components/settings/received-text'
import { SystemTray } from '../components/settings/system-tray/system-tray'
import { TransferHistorySettings } from '../components/settings/transfer-history'
import { useTranslation } from '../i18n'

export function SettingGeneralPage() {
	const { t } = useTranslation()
	return (
		<>
			<MobileSettingSidebar>
				{t('settings.navItems.general')}
			</MobileSettingSidebar>
			<BroadcastSettings />
			<RelayStatusSettings />
			{IS_TAURI && <Notifications />}
			{IS_DESKTOP && <ReceivedTextSettings />}
			{IS_DESKTOP && <SystemTray />}
			{IS_MACOS && <FlashDropSettings />}
			{IS_TAURI && <TransferHistorySettings />}
			{IS_UPDATER_AVAILABLE && <AutoUpdate />}
			<DebugMode />
		</>
	)
}
