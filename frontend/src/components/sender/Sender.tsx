import { ClipboardIcon, FileTextIcon, StopCircleIcon } from 'lucide-react'
import { useEffect } from 'react'
import { IS_PAIRING_CAPABLE } from '@/lib/platform'
import { useSender } from '../../hooks/useSender'
import { useTranslation } from '../../i18n/react-i18next-compat'
import { useSenderStore } from '../../store/sender-store'
import { TransferSuccessScreen } from '../common/TransferSuccessScreen'
import {
	AlertDialog,
	AlertDialogClose,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
} from '../ui/alert-dialog'
import { Button } from '../ui/button'
import { ToggleGroup, ToggleGroupItem } from '../ui/toggle-group'
import { DragDrop } from './DragDrop'
import { ShareActionCard } from './ShareActionCard'
import { SharingActiveCard } from './SharingActiveCard'
import { TextComposer } from './TextComposer'

interface SenderProps {
	isActive: boolean
	onTransferStateChange: (isSharing: boolean) => void
}

export function Sender({ isActive, onTransferStateChange }: SenderProps) {
	const {
		viewState,
		isSharing,
		isTransporting,
		ticket,
		selectedPaths,
		selectedPath,
		pathType,
		isLoading,
		copySuccess,
		alertDialog,
		transferMetadata,
		transferProgress,
		sendMode,
		textDraft,
		isBroadcastMode,
		activeConnectionCount,
		pairedDevices,
		isNodeReady,
		isNodeStatusPending,
		pairedInviteStatus,
		onInvitePairedDevice,
		onInviteNearbyDevice,
		handleFileSelect,
		handleFilesSelect,
		clearSelectedPath,
		removeSelectedPath,
		setSendMode,
		setTextDraft,
		clearTextDraft,
		startSharing,
		stopSharing,
		copyTicket,
		showAlert,
		closeAlert,
		resetForNewTransfer,
	} = useSender()

	const { t } = useTranslation()
	const setIsBroadcastMode = useSenderStore((state) => state.setIsBroadcastMode)

	useEffect(() => {
		onTransferStateChange(isSharing)
	}, [isSharing, onTransferStateChange])

	// Reset broadcast mode to false when idle screen is shown
	useEffect(() => {
		if (viewState === 'IDLE' && isBroadcastMode) {
			setIsBroadcastMode(false)
		}
	}, [viewState, isBroadcastMode, setIsBroadcastMode])

	return (
		<div className="p-0 sm:p-6 space-y-6 relative h-[65dvh] sm:h-112 overflow-y-auto flex flex-col">
			{/* IDLE state: Show file selection UI */}
			{viewState === 'IDLE' && (
				<>
					<div className="text-center">
						<h2 className="text-xl font-semibold mb-2">
							{t('common:sender.title')}
						</h2>
						<p className="text-sm text-muted-foreground">
							{t('common:sender.subtitle')}
						</p>
					</div>
					<ToggleGroup
						value={[sendMode]}
						variant="outline"
						size="sm"
						className="mx-auto"
						onValueChange={(value) => {
							const nextMode = value[0]
							if (nextMode === 'file' || nextMode === 'text') {
								setSendMode(nextMode)
							}
						}}
					>
						<ToggleGroupItem
							value="file"
							aria-label={t('common:sender.text.file')}
						>
							<FileTextIcon />
							{t('common:sender.text.file')}
						</ToggleGroupItem>
						{IS_PAIRING_CAPABLE ? (
							<ToggleGroupItem
								value="text"
								aria-label={t('common:sender.text.text')}
							>
								<ClipboardIcon />
								{t('common:sender.text.text')}
							</ToggleGroupItem>
						) : null}
					</ToggleGroup>
					<div className="space-y-4 flex-1 flex flex-col">
						{sendMode === 'text' ? (
							<TextComposer
								draft={textDraft}
								isActive={isActive}
								isLoading={isLoading}
								onClear={clearTextDraft}
								onDraftChange={setTextDraft}
								onImportError={(description) =>
									showAlert(
										t('common:sender.text.importFailed'),
										description,
										'error'
									)
								}
								onStartSharing={startSharing}
							/>
						) : isActive ? (
							<>
								<DragDrop
									onFileSelect={handleFileSelect}
									onFilesSelect={handleFilesSelect}
									selectedPaths={selectedPaths}
									selectedPath={selectedPath}
									isLoading={isLoading}
									onClearSelection={clearSelectedPath}
									onRemoveSelectedPath={removeSelectedPath}
								/>

								<ShareActionCard
									selectedPaths={selectedPaths}
									selectedPath={selectedPath}
									isLoading={isLoading}
									onStartSharing={startSharing}
								/>
							</>
						) : null}
					</div>
				</>
			)}

			{/* SUCCESS state: Show transfer success screen (only in non-broadcast mode) */}
			{viewState === 'SUCCESS' && transferMetadata && !isBroadcastMode && (
				<div className="flex-1 flex flex-col">
					<TransferSuccessScreen
						metadata={transferMetadata}
						onDone={resetForNewTransfer}
					/>
				</div>
			)}

			{/* SHARING or TRANSPORTING state: Show active sharing UI */}
			{/* In broadcast mode, only show SHARING state (skip TRANSPORTING) */}
			{(((viewState === 'SHARING' || viewState === 'TRANSPORTING') &&
				!isBroadcastMode) ||
				(viewState === 'SHARING' && isBroadcastMode)) && (
				<div className="flex-1 flex flex-col">
					<SharingActiveCard
						isSharing={isSharing}
						isLoading={isLoading}
						isTransporting={isTransporting && !isBroadcastMode}
						isCompleted={false}
						selectedPaths={selectedPaths}
						selectedPath={selectedPath}
						sendMode={sendMode}
						pathType={pathType}
						ticket={ticket}
						copySuccess={copySuccess}
						transferProgress={transferProgress}
						isBroadcastMode={isBroadcastMode}
						activeConnectionCount={activeConnectionCount}
						pairedDevices={pairedDevices}
						isNodeReady={isNodeReady}
						isNodeStatusPending={isNodeStatusPending}
						pairedInviteStatus={pairedInviteStatus}
						onInvitePairedDevice={onInvitePairedDevice}
						onInviteNearbyDevice={onInviteNearbyDevice}
						onStartSharing={startSharing}
						onStopSharing={stopSharing}
						onCopyTicket={copyTicket}
						onSetBroadcast={setIsBroadcastMode}
					/>
				</div>
			)}

			{/* Fallback: Show debug info if no view matches */}
			{viewState !== 'IDLE' &&
				viewState !== 'SUCCESS' &&
				viewState !== 'SHARING' &&
				viewState !== 'TRANSPORTING' && (
					<div className="text-center p-4 border border-red-500">
						<p className="text-red-500 font-bold">
							Unexpected view state: {viewState}
						</p>
						<p className="text-sm">
							isSharing={String(isSharing)}, isTransporting=
							{String(isTransporting)}
						</p>
					</div>
				)}

			<AlertDialog open={alertDialog.isOpen} onOpenChange={closeAlert}>
				<AlertDialogContent>
					<AlertDialogHeader>
						<AlertDialogTitle>{alertDialog.title}</AlertDialogTitle>
						<AlertDialogDescription>
							{alertDialog.description}
						</AlertDialogDescription>
					</AlertDialogHeader>
					<AlertDialogFooter>
						<AlertDialogClose
							onClick={closeAlert}
							render={
								<Button variant="secondary" size="sm">
									{t('common:cancel')}
								</Button>
							}
						/>
						<AlertDialogClose
							onClick={() => {
								stopSharing()
								closeAlert()
							}}
							render={
								<Button size="sm">
									{t('common:sender.stopSharing')}
									<StopCircleIcon />
								</Button>
							}
						/>
					</AlertDialogFooter>
				</AlertDialogContent>
			</AlertDialog>
		</div>
	)
}
