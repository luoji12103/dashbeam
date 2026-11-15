export type SharePreparation<T> = {
	result: Promise<T | null>
	cancel: () => void
	isCancelled: () => boolean
	selectTarget?: (endpointId: string) => boolean
}

/** A fast-send preference does no native preparation until a peer is chosen. */
export function prepareTargetedShare<T>(args: {
	previous?: Promise<unknown>
	prepare: (endpointId: string) => Promise<T>
	dispose: (value: T) => Promise<unknown>
}): SharePreparation<T> {
	let resolveTarget!: (endpointId: string | null) => void
	const target = new Promise<string | null>((resolve) => {
		resolveTarget = resolve
	})
	let selected = false
	const preparation = prepareShare({
		previous: args.previous,
		prepare: async () => {
			const endpointId = await target
			if (endpointId === null || preparation.isCancelled()) {
				throw new Error('cancelled')
			}
			return args.prepare(endpointId)
		},
		dispose: args.dispose,
	})
	return {
		result: preparation.result,
		isCancelled: preparation.isCancelled,
		selectTarget: (endpointId) => {
			if (selected || preparation.isCancelled()) return false
			selected = true
			resolveTarget(endpointId)
			return true
		},
		cancel: () => {
			preparation.cancel()
			resolveTarget(null)
		},
	}
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
