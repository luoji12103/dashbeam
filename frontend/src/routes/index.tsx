import { useEffect, useRef, useState } from 'react'
import {
	Tabs,
	TabsContent,
	TabsList,
	TabsTrigger,
} from '@/components/animate-ui/components/tabs'
import * as SingleLayoutPage from '@/components/common/SingleLayoutPage'
import { Receiver } from '@/components/receiver/Receiver'
import { useReceiverContext } from '@/components/receiver/ReceiverProvider'
import { Sender } from '@/components/sender/Sender'
import { Frame, FrameHeader, FramePanel } from '@/components/ui/frame'
import { toastManager } from '@/components/ui/toast'
import { useTranslation } from '@/i18n'
import { invoke, listen, listenForReceiveLinks } from '@/lib/platform-api'
import { ticketFromReceiveLink } from '@/lib/receive-link'
import { relayFallbackToastDescriptionKey } from '@/lib/relay-fallback-toast'
import { useSenderStore } from '@/store/sender-store'
import { useTransferTabStore } from '@/store/transfer-tab-store'

export function IndexPage() {
	const [activeTab, setActiveTab] = useState<'send' | 'receive'>('send')
	const [isSharing, setIsSharing] = useState(false)
	const [isReceiving, setIsReceiving] = useState(false)
	const isInitialRender = useRef(false)
	const { t } = useTranslation()
	const { handleTicketChange } = useReceiverContext()

	// Store actions
	const addSelectedPaths = useSenderStore((state) => state.addSelectedPaths)
	const setPathType = useSenderStore((state) => state.setPathType)
	const requestedTab = useTransferTabStore((state) => state.requestedTab)
	const clearRequestedTab = useTransferTabStore(
		(state) => state.clearRequestedTab
	)

	useEffect(() => {
		const applyReceiveLink = (url: string) => {
			const ticket = ticketFromReceiveLink(url)
			if (!ticket) return
			handleTicketChange(ticket)
			setActiveTab('receive')
		}

		const unlisten = listenForReceiveLinks(applyReceiveLink)
		return () => {
			void unlisten.then((stop) => stop())
		}
	}, [handleTicketChange])

	useEffect(() => {
		if (requestedTab) {
			setActiveTab(requestedTab)
			clearRequestedTab()
		}
	}, [requestedTab, clearRequestedTab])

	useEffect(() => {
		isInitialRender.current = true

		const applyIntent = async () => {
			const paths = await invoke<string[]>('check_launch_intent')
			if (!paths?.length) return
			const sender = useSenderStore.getState()
			if (
				sender.viewState === 'SHARING' ||
				sender.viewState === 'TRANSPORTING' ||
				sender.isLoading
			) {
				toastManager.add({
					title: '请先完成当前分享，再从 Finder 添加文件',
					type: 'warning',
				})
				return
			}
			setActiveTab('send')
			addSelectedPaths(paths)
			try {
				const path = useSenderStore.getState().selectedPaths[0]
				const type = await invoke<string>('check_path_type', { path })
				setPathType(type as 'file' | 'directory')
			} catch {
				setPathType(null)
			}
		}

		const consume = () =>
			void applyIntent().catch((e) =>
				console.error('Failed to check launch intent:', e)
			)
		const unlistenPromise = listen('launch-intent', consume)
		// Register before draining the cold-start queue to avoid dropping an open event.
		void unlistenPromise.then(consume)

		// Surface the custom->public relay fallback at transfer time so a user who
		// chose "custom for privacy" is not silently put on public relays.
		const unlistenFellBackPromise = listen<string>(
			'relay-fell-back',
			(event) => {
				const descriptionKey = relayFallbackToastDescriptionKey(event.payload)
				if (!descriptionKey) {
					return
				}

				toastManager.add({
					title: t('footer.relay.fellBackToastTitle'),
					description: t(descriptionKey),
					type: 'warning',
				})
			}
		)

		return () => {
			unlistenPromise.then((unlisten) => unlisten())
			unlistenFellBackPromise.then((unlisten) => unlisten())
		}
	}, [addSelectedPaths, setPathType, t])

	// Example: Routes can be accessed at different paths
	// You can navigate using: import { useNavigate } from 'react-router-dom'
	// const navigate = useNavigate(); navigate('/send') or navigate('/receive')

	return (
		<SingleLayoutPage.SingleLayoutPage>
			<div className="max-w-2xl mx-auto w-full pt-8 sm:pt-0">
				<Frame>
					<Tabs
						value={activeTab}
						onValueChange={(v) => setActiveTab(v as 'send' | 'receive')}
					>
						<FrameHeader>
							<TabsList className="w-full">
								<TabsTrigger disabled={isReceiving} value="send">
									{t('common:send')}
								</TabsTrigger>
								<TabsTrigger disabled={isSharing} value="receive">
									{t('common:receive')}
								</TabsTrigger>
							</TabsList>
						</FrameHeader>
						<FramePanel>
							<TabsContent
								forceMount
								value="send"
								className="data-[state=inactive]:hidden"
							>
								<Sender onTransferStateChange={setIsSharing} />
							</TabsContent>
							<TabsContent
								forceMount
								value="receive"
								className="data-[state=inactive]:hidden"
							>
								<Receiver onTransferStateChange={setIsReceiving} />
							</TabsContent>
						</FramePanel>
					</Tabs>
				</Frame>
			</div>
		</SingleLayoutPage.SingleLayoutPage>
	)
}
