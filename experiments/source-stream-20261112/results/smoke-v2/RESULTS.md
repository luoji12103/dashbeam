# Source-stream engine A/B results

6/6 process-pair samples successful; 6/6 expected-path verified; 10 file receives recorded.

Medians below. Payload is an internal positive progress / first stream-frame proxy, NOT a packet-capture first byte. First-transfer end-to-end includes cold share preparation and 10ms ticket polling. No GUI, metadata advisory, device consent or Android provider staging is simulated.

| Case | Version | Valid / attempted | Share ready (s) | Share → payload (s) | Share → export/commit (s) | Share → API return (s) | Sender prepare rchar (MiB) | Sampled app+output peak disk (MiB) |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1. cold-direct-small-lan | legacy | 1/1 | 0.032 | 0.079 | 0.095 | 0.096 | 1.00 | 2.46 |
| 1. cold-direct-small-lan | stream | 1/1 | 0.034 | 0.097 | 0.116 | 0.145 | 0.00 | 1.00 |
| 6. cold-relay-small-wan | legacy | 1/1 | 0.555 | 1.125 | 1.600 | 1.679 | 1.00 | 4.02 |
| 6. cold-relay-small-wan | stream | 1/1 | 0.543 | 1.010 | 1.606 | 1.713 | 0.00 | 1.00 |
| 9. repeat-relay-small-wan | legacy | 1/1 | 0.551 | 1.106 | 1.588 | 1.667 | 1.00 | 4.02 |
| 9. repeat-relay-small-wan | stream | 1/1 | 0.577 | 1.013 | 1.607 | 1.688 | 0.00 | 1.00 |

## Repeated receives

Both versions reuse the same sender share. Stream also keeps its receiver endpoint/connection; legacy download creates a new receiver per call. Each process sample contributes the median of its two repeat receives. These are not new cold user-share measurements.

| Case | Version | Repeat receive → payload (s) | Repeat receive → return (s) |
| --- | --- | ---: | ---: |
| 9 | legacy | 0.529 | 1.088 |
| 9 | stream | 0.165 | 0.447 |

## Resource and interpretation limits

- Source preparation performs zero payload reads on the streaming path (explicit counter checked). Both paths incrementally hash the transmitted bytes; stream does not omit final integrity validation.
- /proc/self/io rchar/write_bytes and getrusage CPU/RSS describe the entire instrumented Linux process, not isolated hardware counters or power consumption. Warm source page cache; rchar counts logical read syscalls, not mmap or physical disk reads.
- Disk is the 10ms sampled sum of unique (device,inode) regular files in task-owned app temp plus output; source fixture excluded, tiny ticket/log files included, hard links counted once. Allocated blocks do not resolve shared reflink extents. Sampled peaks may miss short-lived duplicates and are not a hard upper bound.
- Export/commit is the legacy receive-completed event versus successful stream no-clobber commit. Legacy does not explicitly fsync; stream syncs its file before commit, neither promises parent-directory crash durability. API return includes legacy endpoint close; stream retains endpoint/connection until explicit shutdown and returns after its completion receipt transport acknowledgement. Do not attribute all API-return savings to transfer itself.
- Cold legacy baseline is the already optimized compatible engine, without app advisory metadata. Do not compare these numbers directly against the earlier app-orchestration benchmark.
- Repeated receive byte-comparison occurs after each individual receive timer; aggregate process metrics can include comparison/sampling overhead. No p95, crash durability, production relay auth, phone GUI, unknown-length Android pipe or internet SLO claim.
- Streaming is an explicit experimental single-regular-file API. No legacy ticket or installed client was automatically migrated; no retry/resume or disk spool is provided.
