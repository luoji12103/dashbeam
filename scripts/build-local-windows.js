#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const repoRoot = fileURLToPath(new URL('..', import.meta.url))

const env = {
	...process.env,
	VITE_LOCAL_BUILD: 'true',
	TAURI_ENV_PLATFORM: 'windows',
}

for (const args of [
	['run', 'build:wasm'],
	['run', 'build'],
]) {
	const result = spawnSync('pnpm', args, {
		cwd: repoRoot,
		env,
		stdio: 'inherit',
		shell: process.platform === 'win32',
	})
	if (result.error) throw result.error
	if (result.status !== 0) process.exit(result.status ?? 1)
}
