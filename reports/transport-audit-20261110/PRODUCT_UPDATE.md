# Product latency update — 2026-11-11

This is the first compatible product stage following the research, not the
SESSION2/spool implementation. Upstream was refreshed to
`c5de16552fc946ec9d0ad3b3792875868e826a05`; relative to the audit baseline
`b41eddb16b226667de6f8dd7f1326a0ce6bf5e7d`, only two issue templates changed.
The fork remains on Iroh 1.0.3 / Blobs 0.103.0. No production relay is changed.

## Changes

1. Strict custom relay resolution validates configuration without creating a
   temporary probe endpoint. Explicit Public fallback retains its probe/fallback
   policy; Settings diagnostics still probe when requested.
2. Sender endpoint online/warm-up overlaps legacy content import/hash. Online,
   import, and store failures close resources before removing temporary stores.
3. Both metadata preview and payload download keep the selected discovery
   lookup even if a ticket already has address hints. Custom discovery never
   inherits public lookup services.
4. The first control endpoint reads persisted relay/discovery settings from the
   existing app_settings envelope. Frontend synchronization waits for hydration.
   Malformed saved settings fail closed; repair in Settings and restart the app.
5. Metadata lookup runs in parallel with payload download, not as a three-second
   startup gate. Its bounded wait may still delay final text classification.
   Cancellation reaches both phases and holds the receive reservation until
   cleanup. Completion UI/history events wait for validation/final export.
6. The sender shows preparation and a usable target picker immediately. Legacy
   invitations still require the completed ticket; no early payload is claimed.
   Cross-page preparation coordination and ticket-scoped cleanup prevent stale
   callbacks from overwriting/stopping newer shares.
7. Pairing records store a relay hint observed on the **remote** connection, not
   the local endpoint's home relay. Direct-only connections have no relay hint.

## Validation before packaging

Debian DEV isolated source: `/home/luoji/code/dashbeam-latency-20261111.jVKZD9`.
The existing dirty DEV checkout, settings, pairings, production relays, and
installed applications are not replaced by these tests.

- Frontend: 173/173 tests; TypeScript and darwin production build passed.
- Protocol/native focused tests: 22 passed, including Strict zero-probe,
  explicit Public fallback, lookup isolation, online/import cleanup, and a real
  loopback QUIC download recovered from stale ticket addresses via MemoryLookup.
- App Rust: 98 passed, one isolated-X11 clipboard test ignored, one public-relay
  metadata e2e filtered out. Includes payload-before-metadata, cancellation after
  payload while metadata is pending, completion gating, receive reservation,
  preparation cancellation, persisted network parsing, and existing text tests.
- Cancellation during non-interruptible final export is recorded and suppresses
  late text/success publication. Already exported files are not silently removed.
- Cargo.lock files are unchanged. No tests use real paired recipient devices.

## Packaging and remaining work

Published pre-release `v0.7.1-local.4`: macOS ARM64 ZIP, Windows x64 NSIS + portable ZIP,
Android ARM64 APK, all from one frozen source revision. macOS bundleVersion and
Android versionCode are 70105. Product version remains 0.7.1. Android preserves
`com.dashbeam.android.local` and the existing private local signing key; it can
update this local app, not the differently signed/identified official app.

Build/signature/asset verification and real-device results must be recorded
separately after packaging. Passing unit tests is not a phone-to-Mac, cross-home-
relay, GUI cancellation, notification, or sustained-throughput acceptance.

macOS package completed: ARM64 Mach-O, bundleVersion 70105, identifier
`com.dashbeam.desktop`; embedded Finder Share extension and deep strict ad-hoc
signature verification passed, ZIP integrity passed. SHA256:
`ca56e5baa44f993279e962de5d1e6bbc1c81001281b4b0a457dec32af85a4f1d`.
The optional debug-info stripping helper warned about missing libLLVM; Cargo
and Tauri completed successfully. No toolchain/system changes were made for
that warning. This package has not replaced `/Applications/DashBeam.app`.

Windows x64 CI run `42997084778` completed successfully from the frozen source.
It uploaded NSIS installer and portable ZIP to the same release draft. Asset
digests reported by GitHub (local download/integrity check recorded separately):

- Installer: `68fc88176d6b6726b29aaffe631e59400765860bbbaf524412d381e274367521`.
- Portable: `db0bc9714b45aa50ac1dab0aefb72946c3d68ee89606d91dcf5c7345e766e0d8`.

Both Windows assets were downloaded and their SHA256 matched. Portable ZIP
integrity passed; its `DashBeam.exe` is PE32+ x86-64 and `.portable` is present.
The NSIS bootstrap is a PE32 self-extracting installer (its wrapper architecture
does not identify the packaged application's architecture). Windows GUI and
clipboard transfer have not been run on a Windows machine in this update.

Android ARM64 release APK completed on Debian DEV. Package is
`com.dashbeam.android.local`, versionCode 70105, minSdk 29, targetSdk 36;
only `arm64-v8a` is included. APK v3 signature validation, 16 KiB ZIP alignment,
ZIP integrity and native ELF LOAD alignment (`0x4000`) passed. Its certificate
SHA256 matches the previous local APK:
`fa481ae7c59700dfd66e558c6e7ac528359cbd4b7dfb7f5b35c90d4b491253dc`.
SEND/SEND_MULTIPLE and the local FileProvider authority remain in the packaged
manifest. APK SHA256:
`89944ab430dad1cae60e9f99c0f9389a56785525da6ef0bad44ea0a1a645a28b`.
The cross-worktree Tauri cache omitted generated TauriActivity; the reused
generated file was verified byte-for-byte against the locked Tauri 2.10.1
template after package substitution. No product logic or dependency was changed
for this build repair. No phone installation or real-device transfer was done.

All four assets use product commit `2ef67abf2e100f32db9aca7834397e705250053e`.
The release includes a `SHA256SUMS` asset. Release documentation is committed
separately and does not require rebuilding these frozen-source packages.

Release published at `2026-11-11T10:27:01Z`, non-draft and pre-release, not the
stable/latest release. The tag resolves to the frozen product commit. Publishing
via the release API also generated tag-push events for the inherited generic
`publish` and `android-aab` workflows; both extra runs were cancelled rather
than allowing a second generic build/deployment pipeline to alter these assets.
Run `43003708188` and `43003708688` ended cancelled; both Vercel installation
and deployment steps were skipped. The inherited workflows now skip local-tag
automatic jobs for subsequent releases; explicit Windows local dispatch remains
unchanged. This CI-only follow-up does not change the frozen binaries.

The next architectural stage still needs endpoint/ALPN authorization, streaming
integrity and resumability, source-version semantics, Android SAF changes,
per-recipient consent, and crash-safe encrypted disk-spool quotas/ACKs. The
research prototype is not shipped as production code by this candidate.
