import { useTranslation } from '../../../i18n'
import { useAppSettingStore } from '../../../store/app-setting'
import {
	Frame,
	FrameDescription,
	FrameHeader,
	FramePanel,
	FrameTitle,
} from '../../ui/frame'
import { Switch } from '../../ui/switch'

export function ReceivedTextSettings() {
	const { t } = useTranslation()
	const enabled = useAppSettingStore((state) => state.autoCopyReceivedText)
	const setEnabled = useAppSettingStore(
		(state) => state.setAutoCopyReceivedText
	)

	return (
		<Frame>
			<FrameHeader>
				<FrameTitle>{t('settings.general.receivedText.title')}</FrameTitle>
			</FrameHeader>
			<FramePanel>
				<div className="flex items-center justify-between gap-4">
					<div className="flex-1">
						<FrameTitle>
							{t('settings.general.receivedText.autoCopy')}
						</FrameTitle>
						<FrameDescription>
							{t('settings.general.receivedText.autoCopyDescription')}
						</FrameDescription>
					</div>
					<Switch checked={enabled} onCheckedChange={setEnabled} />
				</div>
			</FramePanel>
		</Frame>
	)
}
