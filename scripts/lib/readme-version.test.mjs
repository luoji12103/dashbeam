import assert from 'node:assert/strict'
import { test } from 'node:test'
import { syncReadmeVersion } from './readme-version.mjs'

test('updates each official artifact naming scheme', () => {
	for (const [oldName, newName] of [
		['DashBeam_0.7.1_x64-setup.exe', 'DashBeam_0.7.2_x64-setup.exe'],
		['DashBeam-v0.7.1-arm64.apk', 'DashBeam-v0.7.2-arm64.apk'],
		['DashBeam-0.7.1-1.x86_64.rpm', 'DashBeam-0.7.2-1.x86_64.rpm'],
	]) {
		assert.equal(syncReadmeVersion(`https://github.com/tonyantony300/dashbeam/releases/download/v0.7.1/${oldName}`, '0.7.2'),
			`https://github.com/tonyantony300/dashbeam/releases/download/v0.7.2/${newName}`)
	}
})

test('preserves immutable fork release links and labels', () => {
	const text = '[DashBeam_0.7.1_x64-setup.exe](https://github.com/luoji12103/dashbeam/releases/download/v0.7.1-local.4/DashBeam_0.7.1_x64-setup.exe)'
	assert.equal(syncReadmeVersion(text, '0.7.2'), text)
})

test('updates official version badge', () => {
	assert.equal(syncReadmeVersion('[badge-version]: https://img.shields.io/badge/version-0.7.1-blue', '0.7.2'),
		'[badge-version]: https://img.shields.io/badge/version-0.7.2-blue')
})
