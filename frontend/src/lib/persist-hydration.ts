type PersistHydration = {
	hasHydrated: () => boolean
	onFinishHydration: (listener: () => void) => () => void
}

/**
 * Subscribe before checking the flag so a hydration that finishes during setup
 * cannot be missed. The returned cleanup also cancels a deferred startup task.
 */
export function afterPersistHydration(
	persist: PersistHydration,
	run: () => void
): () => void {
	let active = true
	let unsubscribe: (() => void) | undefined
	const finish = () => {
		if (!active) return
		active = false
		unsubscribe?.()
		run()
	}
	unsubscribe = persist.onFinishHydration(finish)
	if (!active) unsubscribe()
	if (persist.hasHydrated()) finish()
	return () => {
		active = false
		unsubscribe?.()
	}
}
