import { Check, Copy } from 'lucide-react'
import { useTranslation } from '../../i18n/react-i18next-compat'
import type { ReceivedTextState } from '../../types/receiver'
import { Button } from '../ui/button'
import { Textarea } from '../ui/textarea'

interface ReceivedTextCardProps {
	state: ReceivedTextState
	onCopy: () => Promise<void>
}

export function ReceivedTextCard({ state, onCopy }: ReceivedTextCardProps) {
	const { t } = useTranslation()

	return (
		<div className="w-full space-y-3">
			<Textarea
				value={state.content}
				readOnly
				aria-label={t('common:receiver.receivedText.previewLabel')}
				className="max-h-56 min-h-28 resize-y select-text font-mono text-sm"
			/>
			<div className="flex items-center justify-between gap-3">
				<p className="min-w-0 text-xs text-muted-foreground" role="status">
					{state.copyError
						? t('common:receiver.receivedText.copyFailed', {
								error: state.copyError,
							})
						: state.isCopied
							? t('common:receiver.receivedText.copied')
							: t('common:receiver.receivedText.ready')}
				</p>
				<Button
					type="button"
					size="sm"
					variant="secondary"
					onClick={onCopy}
					disabled={state.isCopying}
				>
					{state.isCopied ? <Check size={14} /> : <Copy size={14} />}
					{t('common:receiver.receivedText.copy')}
				</Button>
			</div>
		</div>
	)
}
