import { createContext, useContext, useEffect, useRef, useState } from 'react'
import { useReceiver, type UseReceiverReturn } from '@/hooks/useReceiver'
import { invoke } from '@/lib/platform-api'
import { ReceivedTextCard } from './ReceivedTextCard'
import { useTranslation } from '../../i18n/react-i18next-compat'
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from '../ui/dialog'

const ReceiverContext = createContext<UseReceiverReturn | null>(null)

/** Keeps receive handlers alive across routes (e.g. settings) so paired invites can be accepted globally. */
export function ReceiverProvider({ children }: { children: React.ReactNode }) {
	const receiver = useReceiver()
	const { t } = useTranslation()
	const [textDialogOpen, setTextDialogOpen] = useState(false)
	const presentedTextIdRef = useRef<string | null>(null)

	useEffect(() => {
		const text = receiver.receivedText
		if (!text) {
			setTextDialogOpen(false)
			return
		}
		// Let transfer errors/conflict notices finish first instead of stacking dialogs.
		if (receiver.alertDialog.isOpen) return
		if (presentedTextIdRef.current === text.resultId) {
			return
		}

		presentedTextIdRef.current = text.resultId
		if (text.isCopied) {
			setTextDialogOpen(false)
			return
		}
		setTextDialogOpen(true)
		// Manual action is required, so reveal the app if it was backgrounded.
		void invoke('focus_main_window').catch((error) => {
			console.warn('Failed to show received text window:', error)
		})
	}, [receiver.alertDialog.isOpen, receiver.receivedText])

	return (
		<ReceiverContext.Provider value={receiver}>
			{children}
			<Dialog open={textDialogOpen} onOpenChange={setTextDialogOpen}>
				<DialogContent className="max-w-lg" centered>
					<DialogHeader>
						<DialogTitle>
							{t('common:receiver.receivedText.dialogTitle')}
						</DialogTitle>
						<DialogDescription>
							{t('common:receiver.receivedText.dialogDescription')}
						</DialogDescription>
					</DialogHeader>
					{receiver.receivedText && (
						<div className="px-6 pb-6">
							<ReceivedTextCard
								state={receiver.receivedText}
								onCopy={receiver.copyReceivedText}
							/>
						</div>
					)}
				</DialogContent>
			</Dialog>
		</ReceiverContext.Provider>
	)
}

export function useReceiverContext(): UseReceiverReturn {
	const context = useContext(ReceiverContext)
	if (!context) {
		throw new Error('useReceiverContext must be used within ReceiverProvider')
	}
	return context
}
