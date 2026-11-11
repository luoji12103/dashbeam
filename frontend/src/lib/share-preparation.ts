export type SharePreparation<T> = {
	result: Promise<T | null>
	cancel: () => void
	isCancelled: () => boolean
}

/**
 * Keep a cancelled legacy preparation in the queue until its backend handle has
 * been cleaned up. A replacement share must not be stopped by the old result.
 */
export function prepareShare<T>(args: {
	previous?: Promise<unknown>
	prepare: () => Promise<T>
	dispose: (value: T) => Promise<unknown>
}): SharePreparation<T> {
	let cancelled = false
	const result = (async () => {
		await args.previous
		if (cancelled) return null
		let value: T
		try {
			value = await args.prepare()
		} catch (error) {
			if (cancelled) return null
			throw error
		}
		if (cancelled) {
			await args.dispose(value)
			return null
		}
		return value
	})()
	return {
		result,
		cancel: () => {
			cancelled = true
		},
		isCancelled: () => cancelled,
	}
}
