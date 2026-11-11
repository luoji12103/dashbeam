# DashBeam latency benchmark: measured summary

30/30 transfers passed byte verification; 30/30 also passed the expected-path check.

Times below are medians in seconds; `payload` means the first internal blobs payload-progress proxy, not first wire byte. `return` includes export, advisory wait and endpoint cleanup, but excludes the separately measured byte-verification pass. All failed attempts remain in the raw denominator.

| Case | Version | Verified / attempted | Share ready | Receive → payload | Share → payload | Share → return |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1. direct-small-lan | candidate | 5/5 | 0.055 | 0.056 | 0.102 | 0.120 |
| 1. direct-small-lan | optimized | 5/5 | 0.039 | 0.050 | 0.098 | 0.116 |
| 3. direct-many-small | candidate | 5/5 | 0.057 | 0.051 | 0.130 | 0.179 |
| 3. direct-many-small | optimized | 5/5 | 0.047 | 0.065 | 0.127 | 0.175 |
| 7. relay-small-wan | candidate | 5/5 | 0.557 | 0.547 | 1.113 | 1.668 |
| 7. relay-small-wan | optimized | 5/5 | 0.559 | 0.554 | 1.135 | 1.691 |

## Paired savings

Positive numbers mean faster than the matched baseline. These are medians of per-pair differences, not a subtraction of the two column medians. Small samples support median/range only; no p95 claims.

| Case | Version | Pairs | Share → payload saved | Difference range | Share → return saved |
| --- | --- | ---: | ---: | --- | ---: |
| 1 | optimized | 5 | 0.011 | 0.002–0.021 | 0.010 |
| 3 | optimized | 5 | -0.009 | -0.014–0.026 | -0.004 |
| 7 | optimized | 5 | -0.009 | -0.030–0.002 | -0.009 |

## Scope

- Frozen source revisions, identical instrumentation, fixed compiler/profile and CPU/memory limits; see environment/build manifests.
- Real Rust engine + simulated application command orchestration. This is not a full GUI/Android SAF/phone-to-Mac test.
- Each sample uses a fresh process, endpoint, ephemeral identity, store and output. Source page cache is equally warmed.
- Direct/relay paths are checked on the real connection.
- One isolated Linux kernel, Docker bridge and local relay.
- Internet routing, GZ↔SH geographic links, NAT varieties, mobile sleep and production authentication are not represented.
- See per-case ping calibration and per-sample qdisc counters. Timing includes a shared-file ticket handoff with a 20 ms polling interval.
- Output API completion is not a power-loss durability/fsync guarantee.
