# DashBeam DEV network benchmark

This compares frozen product revisions `f3253bbbf6c1028821e2a66602ede9557cff4113`
(baseline) and `2ef67abf2e100f32db9aca7834397e705250053e` (`local.4` candidate).
It runs the **real Rust engine with simulated application-command orchestration**,
not the GUI or Android file picker. Both use Iroh 1.0.3 / Blobs 0.103.0.

The full matrix has ten scenarios and five matched old/new pairs per scenario:
100 attempted transfers. Results retain failures, byte checks, and selected-path
checks. No p95/SLO claim is made from five repetitions.

## Scope and isolation

- Run heavy compilation/testing only on `SER8-PVE-VM-102-Debian13-Dev`.
- Actual experiment root: `/home/luoji/code/dashbeam-benchmark-20261111.6mQj0M`.
- Do not reuse or overwrite the existing dirty `~/code/dashbeam-macos` checkout.
  The existing Cargo target is only a shared build cache. Keep at least 10 GiB free.
- Use an internal Docker bridge, two endpoint containers and one relay container.
  Only these containers receive `NET_ADMIN` / `NET_RAW`; no privileged containers,
  host networking, host qdisc or host firewall edits.
- Relay is the official Iroh relay 1.0.3 Linux x64 binary. Downloaded tar SHA256:
  `ad7de882c4825a851b38869a3f1622b674cb65f344304e432f862ddf72d6b39a`.
- Real HTTPS + QUIC address discovery use an ephemeral experiment-only CA.
  `SSL_CERT_FILE` is passed to the benchmark processes only; host trust is untouched.
  Do not commit generated private keys, endpoint tickets or synthetic payloads.
- No production relay, installed application, real identity, pairing, clipboard,
  notification or device setting is changed. Relay authentication is not modeled.
- Every sample has independent processes/endpoints/identities/stores/exports.
  The input page cache is deliberately warmed equally for both versions.
- `run.mjs` validates expected paths at connection and transfer completion.
  Forced-relay scenarios also block peer UDP in both isolated namespaces.
  This is not continuous packet-level path tracing or a NAT-traversal matrix.
- The runner removes only its labeled containers/network, private tickets and
  synthetic exported output. Raw metrics, fixture manifests and sources remain.

## Scenarios

`rtt` is the nominal sender↔receiver path round trip: each endpoint's egress has
half the requested delay. The relay has no extra shaping. The rate cap is on
both endpoint egresses; it is not an assertion of achievable application speed.
Calibration pings and per-sample qdisc before/after counters are saved. Qdisc is
deleted/recreated per sample so counters and seeded loss reset.

| Case | Path | Payload | Nominal RTT | Egress rate | Loss |
| --- | --- | --- | ---: | ---: | ---: |
| 1 | direct | 1 MiB | 2 ms | 1000 Mbit/s | 0 |
| 2 | direct | 64 MiB | 2 ms | 1000 Mbit/s | 0 |
| 3 | direct | 256 × 4 KiB | 2 ms | 1000 Mbit/s | 0 |
| 4 | direct | 1 MiB | 80 ms | 50 Mbit/s | 0 |
| 5 | direct | 64 MiB | 80 ms | 50 Mbit/s | 0 |
| 6 | relay | 1 MiB | 2 ms | 1000 Mbit/s | 0 |
| 7 | relay | 1 MiB | 80 ms | 50 Mbit/s | 0 |
| 8 | relay | 64 MiB | 80 ms | 50 Mbit/s | 0 |
| 9 | relay | 16 MiB | 150 ms | 20 Mbit/s | 0.5% random |
| 10 | relay | 1 MiB | 80 ms | 50 Mbit/s | 0; synthetic advisory stall |

Case 10 sleeps only inside the application's three-second advisory timeout;
it does not delay a real metadata ALPN response. It tests parallel orchestration,
not real-world metadata failure recovery. The candidate still waits for advisory
settlement before its final return, so payload start and final completion differ.

## Reproduction

Use separate `git archive` exports of the two revisions in `baseline-v2/` and
`candidate-v2/`. Copy these scripts to `tools/`. Build the Docker image from
`Dockerfile`, tagged `dashbeam-benchmark:20261111`. Its exact built image digest is
saved in `environment.json`; the base image is digest-pinned.

1. Copy `latency_harness.rs` to each archive's `engine/examples/` and apply
   `node tools/instrument.mjs ABS_ARCHIVE_ROOT` exactly once per archive.
   Instrumentation manifests save before/after SHA256. The harness uses a
   benchmark-only observer exported by instrumentation, so instrumentation is
   required before compiling the example.
2. Compile each with `cargo build --manifest-path ARCHIVE/engine/Cargo.toml
   --locked --offline --release --example latency_harness` and copy the resulting
   binary to `bin/baseline` or `bin/candidate` **before** building the other.
   Fixed environment: `CARGO_BUILD_JOBS=2`, `CARGO_PROFILE_RELEASE_OPT_LEVEL=2`,
   `CARGO_PROFILE_RELEASE_LTO=false`, `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=8`.
   When sharing a target directory, touch every archived engine `.rs` source
   before each build: archive mtimes can otherwise make Cargo reuse the other
   revision's artifact. Require different binary digests and verify source hashes.
3. Install the verified official relay binary as `bin/iroh-relay`. Generate
   fixtures once with `node tools/fixtures.mjs ABS_ROOT/fixtures`. Create temporary
   TLS material once with `sh tools/make-certs.sh ABS_ROOT/tools`.
4. Save `rustc -Vv` and `cargo -V` to `results/rustc.txt` and `results/cargo.txt`.
   Save instrumentation manifests as `results/instrument-baseline-v2.json` and
   `results/instrument-candidate-v2.json`. Then run
   `node tools/record-build.mjs ABS_ROOT`.
5. Smoke: `node tools/run.mjs ABS_ROOT smoke-v2 1 1,6`.
   Full: `node tools/run.mjs ABS_ROOT main-v2 5`.
   A label is create-exclusive; use a new label for every run.
6. `node tools/summarize.mjs ABS_ROOT/results/main-v2` creates `summary.json` and
   `RESULTS.md`. Preserve `samples.jsonl` and all calibration records.

The default relay and pkarr URL is the internal `https://relay.dashbeam.test:3341`.
The pkarr path returns 404; public DNS/discovery is not used. Tickets carry the
actual synthetic endpoint addresses. The relay serves QAD on UDP 7842.

If interrupted by an external signal, inspect and remove only the experiment's
own containers/network after verifying their exact `org.dashbeam.experiment`
labels. Do not use Docker prune, broad process kills or host network resets.

## Timing and interpretation

- `share_ready_ms`: basic metadata construction + configured relay resolution +
  real engine preparation until the legacy ticket is available. No GUI thumbnails,
  human target selection, signaling-server delivery or Android SAF is included.
- `end_to_end_payload_progress_ms`: shared monotonic clock from sender intent to
  the first positive internal Blobs payload-progress observation. This is a
  **progress proxy, not an actual first-wire-byte timestamp**. The normal UI
  progress event is separately recorded and is throttled.
- The shared-file ticket handoff polls every 20 ms and performs an atomic write
  with fsync; its small coordination overhead is included in end-to-end timing.
- `receive_valid_export_ms` / `end_to_end_valid_export_ms` end at the receive
  function's successful return (including advisory and endpoint close). The exact
  exported bytes are then independently compared outside that timed interval.
  These times are not an fsync or power-loss durability guarantee.
- `connect_and_store_ms` also includes store initialization and local-state query.
  `post_get_export_ms` includes collection loading/events/export after get.
  Close and true online/import completion are now separately instrumented.
- Paired savings are medians of the five individual old-minus-new differences.
  They need not equal the difference of column medians. Ranges are provided;
  statistical significance and population percentiles are not asserted.

## Discarded fixture v1

Initial smoke/partial `main` used plain HTTP through socat, with relay QAD disabled
while client defaults still probed UDP 7842. This introduced ~3 s online waits,
~1 s close waits and likely TCP proxy delay. These diagnostic values are **not**
included in formal A/B results or acceleration claims. The corrected HTTPS+QAD
smoke reduced relay sender prepare from ~3.04 s to ~0.29 s and receive close from
~1 s to ~1.3 ms without a product change. This was an experiment-fixture fix.

No product optimization should be made by deleting graceful endpoint close or
disabling address discovery merely to reproduce those artificial gains.
