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

Candidate `v0.7.1-local.4`: macOS ARM64 ZIP, Windows x64 NSIS + portable ZIP,
Android ARM64 APK, all from one frozen source revision. macOS bundleVersion and
Android versionCode are 70105. Product version remains 0.7.1. Android preserves
`com.dashbeam.android.local` and the existing private local signing key; it can
update this local app, not the differently signed/identified official app.

Build/signature/asset verification and real-device results must be recorded
separately after packaging. Passing unit tests is not a phone-to-Mac, cross-home-
relay, GUI cancellation, notification, or sustained-throughput acceptance.

The next architectural stage still needs endpoint/ALPN authorization, streaming
integrity and resumability, source-version semantics, Android SAF changes,
per-recipient consent, and crash-safe encrypted disk-spool quotas/ACKs. The
research prototype is not shipped as production code by this candidate.
