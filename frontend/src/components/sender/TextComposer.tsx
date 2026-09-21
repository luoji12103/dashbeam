import { FileUp, Send, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { getCurrentWindow, invoke, openDialog } from '@/lib/platform-api'
import { IS_DESKTOP } from '@/lib/platform'
import {
	isMarkdownPath,
	MAX_SEND_TEXT_BYTES,
	validateTextDraft,
} from '@/lib/send-text'
import { useTranslation } from '../../i18n/react-i18next-compat'
import { Button } from '../ui/button'
import { Textarea } from '../ui/textarea'
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/tooltip'

interface TextComposerProps {
	draft: string
	isActive: boolean
	isLoading: boolean
	onClear: () => void
	onDraftChange: (text: string) => void
	onImportError: (description: string) => void
	onStartSharing: () => Promise<void>
}

export function TextComposer({
	draft,
	isActive,
	isLoading,
	onClear,
	onDraftChange,
	onImportError,
	onStartSharing,
}: TextComposerProps) {
	const { t } = useTranslation()
	const [isImporting, setIsImporting] = useState(false)
	const [isDragActive, setIsDragActive] = useState(false)
	const importGenerationRef = useRef(0)
	const isMountedRef = useRef(true)
	const validation = validateTextDraft(draft)

	useEffect(() => {
		isMountedRef.current = true
		return () => {
			isMountedRef.current = false
			importGenerationRef.current += 1
		}
	}, [])

	const importMarkdown = useCallback(
		async (path: string) => {
			if (!isMountedRef.current || isLoading || isImporting) return
			if (!isMarkdownPath(path)) {
				onImportError(t('common:sender.text.importMarkdownOnly'))
				return
			}

			const generation = importGenerationRef.current + 1
			importGenerationRef.current = generation
			setIsImporting(true)
			try {
				const content = await invoke<string>('read_text_file', { path })
				if (
					!isMountedRef.current ||
					importGenerationRef.current !== generation
				) {
					return
				}
				const imported = validateTextDraft(content)
				if (imported.issue === 'too-large') {
					onImportError(t('common:sender.text.tooLarge'))
					return
				}
				onDraftChange(content)
			} catch (error) {
				if (
					isMountedRef.current &&
					importGenerationRef.current === generation
				) {
					onImportError(String(error))
				}
			} finally {
				if (
					isMountedRef.current &&
					importGenerationRef.current === generation
				) {
					setIsImporting(false)
				}
			}
		},
		[isImporting, isLoading, onDraftChange, onImportError, t]
	)

	const chooseMarkdown = useCallback(async () => {
		if (!IS_DESKTOP || !isMountedRef.current || isLoading || isImporting) return
		try {
			const selected = await openDialog({
				multiple: false,
				directory: false,
				filters: [{ name: 'Markdown', extensions: ['md'] }],
			})
			const path = Array.isArray(selected) ? selected[0] : selected
			if (path) {
				await importMarkdown(path)
			}
		} catch (error) {
			if (isMountedRef.current) {
				onImportError(String(error))
			}
		}
	}, [importMarkdown, isImporting, isLoading, onImportError])

	useEffect(() => {
		if (!isActive || !IS_DESKTOP || isLoading || isImporting) return

		let disposed = false
		let unlistenDrop: (() => void) | undefined
		let unlistenHover: (() => void) | undefined
		let unlistenLeave: (() => void) | undefined

		const setupListeners = async () => {
			const appWindow = await getCurrentWindow()
			if (disposed) return

			const drop = await appWindow.listen<{
				paths: string[]
			}>('tauri://drag-drop', (event) => {
				setIsDragActive(false)
				if (isLoading || isImporting) return
				const paths = event.payload?.paths ?? []
				if (paths.length !== 1 || !paths[0]) {
					onImportError(t('common:sender.text.importMarkdownOnly'))
					return
				}
				void importMarkdown(paths[0])
			})
			if (disposed) {
				drop()
				return
			}
			unlistenDrop = drop

			const hover = await appWindow.listen('tauri://drag-hover', () => {
				setIsDragActive(true)
			})
			if (disposed) {
				hover()
				return
			}
			unlistenHover = hover

			const leave = await appWindow.listen('tauri://drag-leave', () => {
				setIsDragActive(false)
			})
			if (disposed) {
				leave()
				return
			}
			unlistenLeave = leave
		}

		void setupListeners().catch((error) => {
			onImportError(String(error))
		})

		return () => {
			disposed = true
			setIsDragActive(false)
			unlistenDrop?.()
			unlistenHover?.()
			unlistenLeave?.()
		}
	}, [importMarkdown, isActive, isImporting, isLoading, onImportError, t])

	return (
		<div
			className={`select-text space-y-3 rounded-lg border p-3 transition-colors sm:p-4 ${
				isDragActive ? 'border-primary bg-primary/5' : 'border-input'
			}`}
		>
			<div className="flex items-center justify-between gap-3">
				<Button
					type="button"
					variant="outline"
					size="sm"
					disabled={isLoading || isImporting}
					onClick={() => void chooseMarkdown()}
				>
					<FileUp />
					{t('common:sender.text.importMarkdown')}
				</Button>
				{draft.length > 0 ? (
					<Tooltip>
						<TooltipTrigger
							render={
								<Button
									type="button"
									variant="ghost"
									size="icon-sm"
									disabled={isLoading || isImporting}
									onClick={onClear}
									aria-label={t('common:sender.text.clear')}
								>
									<Trash2 />
								</Button>
							}
						/>
						<TooltipContent>{t('common:sender.text.clear')}</TooltipContent>
					</Tooltip>
				) : null}
			</div>

			<Textarea
				value={draft}
				onChange={(event) => onDraftChange(event.target.value)}
				placeholder={t('common:sender.text.placeholder')}
				aria-label={t('common:sender.text.label')}
				disabled={isLoading || isImporting}
				spellCheck={false}
				className="min-h-52 resize-y font-mono text-sm"
			/>

			<div className="flex items-center justify-between gap-3">
				<p
					className={`text-xs ${
						validation.issue === 'too-large'
							? 'text-destructive'
							: 'text-muted-foreground'
					}`}
					role="status"
				>
					{validation.issue === 'too-large'
						? t('common:sender.text.tooLarge')
						: t('common:sender.text.size', {
								bytes: validation.byteLength,
								limit: MAX_SEND_TEXT_BYTES,
							})}
				</p>
				<Button
					type="button"
					disabled={!validation.isValid || isLoading || isImporting}
					onClick={() => void onStartSharing()}
				>
					<Send />
					{isLoading
						? t('common:sender.startingShare')
						: t('common:sender.startSharing')}
				</Button>
			</div>
		</div>
	)
}
