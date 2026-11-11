# DashBeam latency benchmark: measured summary

100/100 transfers passed byte verification; 100/100 also passed the expected-path check.

Times below are medians in seconds; `payload` means the first internal blobs payload-progress proxy, not first wire byte. `return` includes export, advisory wait and endpoint cleanup, but excludes the separately measured byte-verification pass. All failed attempts remain in the raw denominator.

| Case | Version | Verified / attempted | Share ready | Receive → payload | Share → payload | Share → return |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1. direct-small-lan | baseline | 5/5 | 0.049 | 0.049 | 0.114 | 0.131 |
| 1. direct-small-lan | candidate | 5/5 | 0.039 | 0.045 | 0.097 | 0.113 |
| 2. direct-large-lan | candidate | 5/5 | 0.081 | 0.047 | 0.136 | 0.749 |
| 2. direct-large-lan | baseline | 5/5 | 0.073 | 0.052 | 0.136 | 0.751 |
| 3. direct-many-small | baseline | 5/5 | 0.059 | 0.064 | 0.137 | 0.183 |
| 3. direct-many-small | candidate | 5/5 | 0.047 | 0.047 | 0.113 | 0.156 |
| 4. direct-small-wan | candidate | 5/5 | 0.042 | 0.407 | 0.469 | 1.029 |
| 4. direct-small-wan | baseline | 5/5 | 0.045 | 0.660 | 0.719 | 1.277 |
| 5. direct-large-wan | baseline | 5/5 | 0.074 | 0.674 | 0.774 | 12.505 |
| 5. direct-large-wan | candidate | 5/5 | 0.066 | 0.402 | 0.485 | 12.223 |
| 6. relay-small-lan | candidate | 5/5 | 0.286 | 0.051 | 0.350 | 0.382 |
| 6. relay-small-lan | baseline | 5/5 | 0.530 | 0.336 | 0.889 | 0.912 |
| 7. relay-small-wan | baseline | 5/5 | 1.140 | 1.506 | 2.662 | 3.221 |
| 7. relay-small-wan | candidate | 5/5 | 0.563 | 0.555 | 1.129 | 1.691 |
| 8. relay-large-wan | candidate | 5/5 | 0.557 | 0.549 | 1.122 | 13.178 |
| 8. relay-large-wan | baseline | 5/5 | 1.140 | 1.506 | 2.663 | 14.716 |
| 9. relay-lossy-slow | baseline | 5/5 | 1.674 | 2.588 | 4.261 | 37.605 |
| 9. relay-lossy-slow | candidate | 5/5 | 0.796 | 0.992 | 1.791 | 34.019 |
| 10. relay-metadata-stall | candidate | 5/5 | 0.561 | 0.545 | 1.122 | 3.574 |
| 10. relay-metadata-stall | baseline | 5/5 | 1.125 | 4.124 | 5.270 | 5.830 |

## Paired savings

Positive numbers mean faster than the matched baseline. These are medians of per-pair differences, not a subtraction of the two column medians. Small samples support median/range only; no p95 claims.

| Case | Version | Pairs | Share → payload saved | Difference range | Share → return saved |
| --- | --- | ---: | ---: | --- | ---: |
| 1 | candidate | 5 | 0.012 | -0.026–0.042 | 0.013 |
| 2 | candidate | 5 | -0.000 | -0.019–0.020 | 0.001 |
| 3 | candidate | 5 | 0.014 | 0.012–0.057 | 0.019 |
| 4 | candidate | 5 | 0.254 | 0.232–0.267 | 0.246 |
| 5 | candidate | 5 | 0.257 | 0.211–0.302 | 0.268 |
| 6 | candidate | 5 | 0.529 | 0.487–0.551 | 0.530 |
| 7 | candidate | 5 | 1.536 | 1.508–1.552 | 1.536 |
| 8 | candidate | 5 | 1.542 | 1.528–1.570 | 1.538 |
| 9 | candidate | 5 | 2.476 | 2.459–2.860 | 3.519 |
| 10 | candidate | 5 | 4.148 | 4.133–4.160 | 2.257 |

## Scope

- Frozen source revisions, identical instrumentation, fixed compiler/profile and CPU/memory limits; see environment/build manifests.
- Real Rust engine + simulated application command orchestration. This is not a full GUI/Android SAF/phone-to-Mac test.
- Each sample uses a fresh process, endpoint, ephemeral identity, store and output. Source page cache is equally warmed.
- Direct/relay paths are checked on the real connection. Case 10 injects advisory-only sleep and is a synthetic timeout stress case.
- One isolated Linux kernel, Docker bridge and local relay. Internet routing, GZ↔SH relays, NAT varieties, mobile sleep and production authentication are not represented.
- See per-case ping calibration and per-sample qdisc counters. Timing includes a shared-file ticket handoff with a 20 ms polling interval.
- Output API completion is not a power-loss durability/fsync guarantee.
