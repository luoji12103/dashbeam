// Advance official links only; fork release links are immutable historical assets.
export function syncReadmeVersion(readme, version) {
	return readme.replace(
		/https:\/\/github\.com\/tonyantony300\/dashbeam\/releases\/download\/v[\d.]+\/[^\s'"<>\)]+/g,
		url => url.replace(/\/download\/v[\d.]+\//, `/download/v${version}/`)
			.replace(/DashBeam_[\d.]+_/g, `DashBeam_${version}_`)
			.replace(/DashBeam-v[\d.]+-/g, `DashBeam-v${version}-`)
			.replace(/DashBeam-[\d.]+-1\./g, `DashBeam-${version}-1.`)
	).replace(
		/\[badge-version\]:\s*https:\/\/img\.shields\.io\/badge\/version-[\d.]+-blue/g,
		`[badge-version]: https://img.shields.io/badge/version-${version}-blue`
	)
}
