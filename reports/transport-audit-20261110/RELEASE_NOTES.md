# DashBeam 0.7.1-local.4 — compatible latency improvements

Community fork of tonyantony300/dashbeam, retaining AGPL-3.0. Product source:
`2ef67abf2e100f32db9aca7834397e705250053e` on `local/macos-integration`.

## What's changed

- Strict custom relays no longer create a temporary preflight endpoint for
  each share/download. Explicit public fallback and Settings diagnostics keep
  their existing behavior.
- Sending endpoint warm-up overlaps file import/hash; stale ticket addresses
  can recover using the configured discovery service.
- The control node uses saved relay/discovery settings from its first start;
  frontend synchronization waits for persisted settings to hydrate.
- Receive metadata lookup and payload download run concurrently. Completion
  waits for validation/final export. Cancellation and ticket-scoped cleanup
  cannot erase or stop a newer transfer.
- The target picker is usable during preparation, with explicit preparing
  state; preparation survives page navigation. Legacy invitations still wait
  for a valid ticket.
- Includes the earlier macOS notification actions to copy received files or
  paths, Finder Share extension, menu bar/Flash Drop integrations, and existing
  cross-platform text/clipboard support.

## Packages and upgrade boundaries

- macOS ARM64 ZIP: bundleVersion 70105, `com.dashbeam.desktop`, ad-hoc signed,
  not notarized. Quit the old app before replacing it. Finder Share extension
  must remain embedded when copying the whole app bundle.
- Windows x64 NSIS installer and portable ZIP: unsigned local packages,
  existing desktop app-data namespace. Portable requires WebView2 Runtime.
- Android ARM64 APK: versionCode 70105, existing local signing identity and
  `com.dashbeam.android.local`. Install over the **existing local app** without
  uninstalling it to preserve its data; this cannot replace the official
  `com.dashbeam.android` application or inherit that application's data.

## Verified scope

173 frontend tests; 22 focused protocol/native tests (including real loopback
QUIC stale-address recovery); 98 app Rust tests passed. One isolated-X11
clipboard test was ignored and one public-relay metadata e2e was excluded.
Cargo.lock files unchanged. Platform package/signature results and SHA256
checksums accompany the final assets.

This is not the new no-prehash streaming protocol or encrypted **disk** spool.
Legacy full-file hashing and Android SAF staging remain. No universal
three-second startup or throughput guarantee is claimed. New phone-to-Mac,
cross-home-relay, Windows GUI, Android background and sustained-load acceptance
still require real-device checks; unit tests/builds do not establish them.

Malformed saved network settings fail closed. Repair them in Settings and
restart DashBeam. Cancellation during a non-interruptible final export prevents
late text/success publication but does not delete files already exported.
