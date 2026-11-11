# DashBeam latency benchmark: measured summary

10/10 transfers passed byte verification; 10/10 also passed the expected-path check.

Times below are medians in seconds; `payload` means the first internal blobs payload-progress proxy, not first wire byte. `return` includes export, advisory wait and endpoint cleanup, but excludes the separately measured byte-verification pass. All failed attempts remain in the raw denominator.

| Case | Version | Verified / attempted | Share ready | Receive → payload | Share → payload | Share → return |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 11. different-explicit-relay-homes | baseline | 5/5 | 1.125 | 1.502 | 2.643 | 5.936 |
| 11. different-explicit-relay-homes | candidate | 5/5 | 0.562 | 0.548 | 1.118 | 4.420 |

## Paired savings

Positive numbers mean faster than the matched baseline. These are medians of per-pair differences, not a subtraction of the two column medians. Small samples support median/range only; no p95 claims.

| Case | Version | Pairs | Share → payload saved | Difference range | Share → return saved |
| --- | --- | ---: | ---: | --- | ---: |
| 11 | candidate | 5 | 1.528 | 1.487–1.558 | 1.524 |

## Scope

- Frozen source revisions, identical instrumentation, fixed compiler/profile and CPU/memory limits; see environment/build manifests.
- Real Rust engine + simulated application command orchestration. This is not a full GUI/Android SAF/phone-to-Mac test.
- Each sample uses a fresh process, endpoint, ephemeral identity, store and output. Source page cache is equally warmed.
- Direct/relay paths are checked on the real connection.
- One isolated Linux kernel and Docker bridge; case 11 uses two independent local relays, with sender configured only A and receiver only B. Home snapshots and selected relay path are verified, not a continuously traced A→B payload route. Both-URL automatic home selection is not tested.
- Internet routing, GZ↔SH geographic links, NAT varieties, mobile sleep and production authentication are not represented.
- See per-case ping calibration and per-sample qdisc counters. Timing includes a shared-file ticket handoff with a 20 ms polling interval.
- Output API completion is not a power-loss durability/fsync guarantee.
