import { Square } from 'lucide-react'
import { useTranslation } from '../../i18n/react-i18next-compat'
import { StatusIndicator } from '../common/StatusIndicator'
import { Button } from '../ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/tooltip'
import type { SenderMode } from '../../store/sender-store'

interface SharingActiveHeaderProps {
	selectedPaths: string[]
	selectedPath: string | null
	sendMode: SenderMode
	statusText: string
	isCompleted: boolean
	isTransporting: boolean
	activeConnectionCount: number
	isBroadcastMode: boolean
	onStopSharing: () => Promise<void>
}

export function SharingActiveHeader({
	selectedPaths,
	selectedPath,
	sendMode,
	statusText,
	isCompleted,
	isTransporting,
	activeConnectionCount,
	isBroadcastMode,
	onStopSharing,
}: SharingActiveHeaderProps) {
	const { t } = useTranslation()

	return (
		<div className="flex w-full items-start justify-between gap-3">
			<div className="min-w-0">
				{sendMode === 'text' ? (
					<p className="text-xs mb-2 max-w-40 sm:max-w-96 truncate text-left">
						<strong className="mr-1">{t('common:sender.text.label')}</strong>{' '}
						{t('common:sender.text.clipboardText')}
					</p>
				) : (
					<Tooltip disabled={!selectedPath && selectedPaths.length <= 1}>
						<TooltipTrigger>
							<p className="text-xs mb-2 max-w-40 sm:max-w-96 truncate text-left">
								<strong className="mr-1">{t('common:sender.fileLabel')}</strong>{' '}
								{selectedPaths.length > 1
									? t('common:sender.multipleFilesSelected', {
											name: selectedPaths[0]?.split('/').pop() || '',
											count: selectedPaths.length - 1,
										})
									: selectedPath?.split('/').pop()}
							</p>
						</TooltipTrigger>
						<TooltipContent className="max-w-xs" side="inline-end">
							<ul className="list-disc pl-4 text-left max-h-60 overflow-auto">
								{selectedPaths.map((path) => (
									<li key={path} className="text-xs">
										{path.split('/').pop()}
									</li>
								))}
							</ul>
						</TooltipContent>
					</Tooltip>
				)}

				<StatusIndicator
					isCompleted={isCompleted}
					isTransporting={isTransporting}
					statusText={statusText}
					activeConnectionCount={activeConnectionCount}
					isBroadcastMode={isBroadcastMode}
				/>
			</div>

			<Button
				size="icon-lg"
				type="button"
				onClick={onStopSharing}
				variant="destructive-outline"
				className="shrink-0 rounded-full font-medium transition-colors not-disabled:not-active:not-data-pressed:before:shadow-none dark:not-disabled:before:shadow-none dark:not-disabled:not-active:not-data-pressed:before:shadow-none"
				aria-label={t('common:sender.stopSharing')}
			>
				<Square className="w-4 h-4" fill="currentColor" />
			</Button>
		</div>
	)
}
