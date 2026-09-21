import { useCallback, useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { decideAndroidTextShare } from '@/lib/android-text-share-policy'
import { IS_ANDROID } from '@/lib/platform'
import { invoke } from '@/lib/platform-api'
import { useSenderStore } from '@/store/sender-store'
import { useTransferTabStore } from '@/store/transfer-tab-store'
import { useTranslation } from '../../i18n/react-i18next-compat'
import { Button } from '../ui/button'
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from '../ui/dialog'
import { toastManager } from '../ui/toast'

type SharedText = {
	text?: string
	error?: string
}

/**
 * Keeps Android ACTION_SEND text separate from the file share bridge. Native
 * retains an unconsumed item while a transfer is active, so this component can
 * wait until the sender is safely idle before it touches the draft.
 */
export function AndroidTextShareBridge() {
	const { t } = useTranslation()
	const navigate = useNavigate()
	const viewState = useSenderStore((state) => state.viewState)
	const isLoading = useSenderStore((state) => state.isLoading)
	const [pendingText, setPendingText] = useState<string | null>(null)
	const [deferredText, setDeferredText] = useState<string | null>(null)
	const pendingTextRef = useRef<string | null>(null)
	const deferredTextRef = useRef<string | null>(null)
	const consumeInFlightRef = useRef(false)
	pendingTextRef.current = pendingText
	deferredTextRef.current = deferredText

	const applyText = useCallback(
		(text: string) => {
			const sender = useSenderStore.getState()
			sender.setSendMode('text')
			sender.setTextDraft(text)
			useTransferTabStore.getState().requestTab('send')
			navigate('/')
		},
		[navigate]
	)

	const setDeferred = useCallback((text: string | null) => {
		deferredTextRef.current = text
		setDeferredText(text)
	}, [])

	const clearPending = useCallback(() => {
		pendingTextRef.current = null
		setPendingText(null)
	}, [])

	const handleSharedText = useCallback(
		(text: string) => {
			const sender = useSenderStore.getState()
			const decision = decideAndroidTextShare({
				text,
				draft: sender.textDraft,
				isTransferActive: sender.isLoading || sender.viewState !== 'IDLE',
			})
			switch (decision.action) {
				case 'apply':
					applyText(text)
					return
				case 'confirm-replace':
					pendingTextRef.current = text
					setPendingText(text)
					useTransferTabStore.getState().requestTab('send')
					navigate('/')
					return
				case 'defer':
					setDeferred(text)
					return
				case 'reject':
					toastManager.add({
						title:
							decision.issue === 'too-large'
								? t('common:sender.text.tooLarge')
								: t('common:sender.text.sharedTextInvalid'),
						type: 'error',
					})
			}
		},
		[applyText, navigate, setDeferred, t]
	)

	const replacePendingText = useCallback(() => {
		const text = pendingTextRef.current
		clearPending()
		if (!text) return

		const sender = useSenderStore.getState()
		const decision = decideAndroidTextShare({
			text,
			draft: sender.textDraft,
			isTransferActive: sender.isLoading || sender.viewState !== 'IDLE',
		})
		if (decision.action === 'defer') {
			setDeferred(text)
			return
		}
		if (decision.action === 'reject') {
			toastManager.add({
				title:
					decision.issue === 'too-large'
						? t('common:sender.text.tooLarge')
						: t('common:sender.text.sharedTextInvalid'),
				type: 'error',
			})
			return
		}
		// The user explicitly chose Replace, so either a still-present or a
		// concurrently edited draft may now be replaced.
		applyText(text)
	}, [applyText, clearPending, setDeferred, t])

	const consumeSharedText = useCallback(async () => {
		if (
			!IS_ANDROID ||
			pendingText !== null ||
			pendingTextRef.current ||
			consumeInFlightRef.current ||
			document.visibilityState !== 'visible'
		)
			return
		if (isLoading || viewState !== 'IDLE') return

		consumeInFlightRef.current = true
		try {
			if (deferredText !== null || deferredTextRef.current !== null) {
				const deferred = deferredTextRef.current
				if (deferred === null) return
				setDeferred(null)
				handleSharedText(deferred)
				return
			}

			const shared = await invoke<SharedText | null>(
				'plugin:native-utils|consume_shared_text'
			)
			if (!shared) return
			if (shared.error) {
				toastManager.add({
					title: t('common:sender.text.sharedTextInvalid'),
					description: shared.error,
					type: 'error',
				})
				return
			}

			const text = shared.text
			if (typeof text !== 'string') return
			handleSharedText(text)
		} catch (error) {
			console.error('Failed to consume Android shared text:', error)
			toastManager.add({
				title: t('common:sender.text.sharedTextInvalid'),
				description: String(error),
				type: 'error',
			})
		} finally {
			consumeInFlightRef.current = false
		}
	}, [
		deferredText,
		handleSharedText,
		isLoading,
		pendingText,
		setDeferred,
		t,
		viewState,
	])

	useEffect(() => {
		if (!IS_ANDROID) return

		let disposed = false
		let unlisten: (() => void) | undefined
		const run = () => {
			if (!disposed) void consumeSharedText()
		}
		const setup = async () => {
			run()
			const { addPluginListener } = await import('@tauri-apps/api/core')
			const listener = await addPluginListener(
				'native-utils',
				'textShareReceived',
				run
			)
			if (disposed) {
				void listener.unregister()
				return
			}
			unlisten = () => void listener.unregister()
		}

		void setup().catch((error) => {
			console.error('Failed to listen for Android shared text:', error)
		})
		window.addEventListener('focus', run)
		document.addEventListener('visibilitychange', run)

		return () => {
			disposed = true
			unlisten?.()
			window.removeEventListener('focus', run)
			document.removeEventListener('visibilitychange', run)
		}
	}, [consumeSharedText])

	if (!IS_ANDROID) return null

	return (
		<Dialog
			open={pendingText !== null}
			onOpenChange={(open) => {
				if (!open) clearPending()
			}}
		>
			<DialogContent centered>
				<DialogHeader>
					<DialogTitle>{t('common:sender.text.replaceDraftTitle')}</DialogTitle>
					<DialogDescription>
						{t('common:sender.text.replaceDraftDescription')}
					</DialogDescription>
				</DialogHeader>
				<DialogFooter>
					<Button variant="secondary" onClick={clearPending}>
						{t('common:sender.text.keepDraft')}
					</Button>
					<Button onClick={replacePendingText}>
						{t('common:sender.text.replaceDraft')}
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	)
}
