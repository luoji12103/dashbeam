# Source-stream engine A/B results

60/60 process-pair samples successful; 60/60 expected-path verified; 84 file receives recorded.

Medians below. Payload is an internal positive progress / first stream-frame proxy, NOT a packet-capture first byte. First-transfer end-to-end includes cold share preparation and 10ms ticket polling. No GUI, metadata advisory, device consent or Android provider staging is simulated.

| Case | Version | Valid / attempted | Share ready (s) | Share → payload (s) | Share → export/commit (s) | Share → API return (s) | Sender prepare rchar (MiB) | Sampled app+output peak disk (MiB) |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1. cold-direct-small-lan | legacy | 3/3 | 0.034 | 0.082 | 0.099 | 0.100 | 1.00 | 2.74 |
| 1. cold-direct-small-lan | stream | 3/3 | 0.026 | 0.068 | 0.087 | 0.115 | 0.00 | 1.00 |
| 2. cold-direct-large-lan | legacy | 3/3 | 0.176 | 0.211 | 4.634 | 4.634 | 256.00 | 514.46 |
| 2. cold-direct-large-lan | stream | 3/3 | 0.038 | 0.091 | 2.477 | 2.506 | 0.00 | 256.00 |
| 3. cold-direct-small-wan | legacy | 3/3 | 0.032 | 0.437 | 0.917 | 0.996 | 1.00 | 4.02 |
| 3. cold-direct-small-wan | stream | 3/3 | 0.028 | 0.311 | 0.916 | 1.021 | 0.00 | 1.00 |
| 4. cold-direct-large-wan | legacy | 3/3 | 0.071 | 0.476 | 12.168 | 12.209 | 64.00 | 130.55 |
| 4. cold-direct-large-wan | stream | 3/3 | 0.057 | 0.367 | 12.133 | 12.239 | 0.00 | 64.00 |
| 5. cold-relay-small-lan | legacy | 3/3 | 0.287 | 0.367 | 0.386 | 0.387 | 1.00 | 2.93 |
| 5. cold-relay-small-lan | stream | 3/3 | 0.289 | 0.335 | 0.361 | 0.363 | 0.00 | 1.00 |
| 6. cold-relay-small-wan | legacy | 3/3 | 0.561 | 1.118 | 1.597 | 1.676 | 1.00 | 4.02 |
| 6. cold-relay-small-wan | stream | 3/3 | 0.569 | 1.007 | 1.604 | 1.711 | 0.00 | 1.00 |
| 7. cold-relay-large-wan | legacy | 3/3 | 0.553 | 1.112 | 13.121 | 13.163 | 64.00 | 130.55 |
| 7. cold-relay-large-wan | stream | 3/3 | 0.560 | 0.996 | 13.073 | 13.170 | 0.00 | 64.00 |
| 8. cold-relay-lossy | legacy | 3/3 | 0.800 | 1.846 | 32.916 | 33.056 | 16.00 | 34.15 |
| 8. cold-relay-lossy | stream | 3/3 | 0.804 | 1.605 | 29.050 | 29.201 | 0.00 | 16.00 |
| 9. repeat-relay-small-wan | legacy | 3/3 | 0.567 | 1.119 | 1.596 | 1.675 | 1.00 | 4.02 |
| 9. repeat-relay-small-wan | stream | 3/3 | 0.559 | 1.009 | 1.593 | 1.694 | 0.00 | 1.00 |
| 10. repeat-direct-small-wan | legacy | 3/3 | 0.043 | 0.454 | 0.932 | 1.011 | 1.00 | 4.02 |
| 10. repeat-direct-small-wan | stream | 3/3 | 0.038 | 0.332 | 0.922 | 1.014 | 0.00 | 1.00 |

## Repeated receives

Both versions reuse the same sender share. Stream also keeps its receiver endpoint/connection; legacy download creates a new receiver per call. Each process sample contributes the median of its two repeat receives. These are not new cold user-share measurements.

| Case | Version | Repeat receive → payload (s) | Repeat receive → return (s) |
| --- | --- | ---: | ---: |
| 9 | legacy | 0.527 | 1.087 |
| 9 | stream | 0.166 | 0.434 |
| 10 | legacy | 0.381 | 0.938 |
| 10 | stream | 0.164 | 0.449 |

## Resource and interpretation limits

- Source preparation performs zero payload reads on the streaming path (explicit counter checked). Both paths incrementally hash the transmitted bytes; stream does not omit final integrity validation.
- /proc/self/io rchar/write_bytes and getrusage CPU/RSS describe the entire instrumented Linux process, not isolated hardware counters or power consumption. Warm source page cache; rchar counts logical read syscalls, not mmap or physical disk reads.
- Disk is the 10ms sampled sum of unique (device,inode) regular files in task-owned app temp plus output; source fixture excluded, tiny ticket/log files included, hard links counted once. Allocated blocks do not resolve shared reflink extents. Sampled peaks may miss short-lived duplicates and are not a hard upper bound.
- Export/commit is the legacy receive-completed event versus successful stream no-clobber commit. Legacy does not explicitly fsync; stream syncs its file before commit, neither promises parent-directory crash durability. API return includes legacy endpoint close; stream retains endpoint/connection until explicit shutdown and returns after its completion receipt transport acknowledgement. Do not attribute all API-return savings to transfer itself.
- Cold legacy baseline is the already optimized compatible engine, without app advisory metadata. Do not compare these numbers directly against the earlier app-orchestration benchmark.
- Repeated receive byte-comparison occurs after each individual receive timer; aggregate process metrics can include comparison/sampling overhead. No p95, crash durability, production relay auth, phone GUI, unknown-length Android pipe or internet SLO claim.
- Streaming is an explicit experimental single-regular-file API. No legacy ticket or installed client was automatically migrated; no retry/resume or disk spool is provided.
