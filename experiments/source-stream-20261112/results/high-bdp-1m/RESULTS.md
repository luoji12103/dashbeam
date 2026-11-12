# Source-stream engine A/B results

8/8 process-pair samples successful; 8/8 expected-path verified; 8 file receives recorded.

Medians below. Payload is an internal positive progress / first stream-frame proxy, NOT a packet-capture first byte. First-transfer end-to-end includes cold share preparation and 10ms ticket polling. No GUI, metadata advisory, device consent or Android provider staging is simulated.

| Case | Version | Valid / attempted | Share ready (s) | Share → payload (s) | Share → export/commit (s) | Share → API return (s) | Sender prepare rchar (MiB) | Sampled app+output peak disk (MiB) |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 11. high-bdp-direct | legacy | 2/2 | 0.066 | 0.467 | 5.711 | 5.731 | 64.00 | 128.73 |
| 11. high-bdp-direct | stream | 2/2 | 0.025 | 0.250 | 6.627 | 6.733 | 0.00 | 64.00 |
| 12. high-bdp-relay | legacy | 2/2 | 0.547 | 1.087 | 6.373 | 6.415 | 64.00 | 130.55 |
| 12. high-bdp-relay | stream | 2/2 | 0.552 | 0.921 | 7.312 | 7.418 | 0.00 | 64.00 |

## Repeated receives

Both versions reuse the same sender share. Stream also keeps its receiver endpoint/connection; legacy download creates a new receiver per call. Each process sample contributes the median of its two repeat receives. These are not new cold user-share measurements.

| Case | Version | Repeat receive → payload (s) | Repeat receive → return (s) |
| --- | --- | ---: | ---: |

## Resource and interpretation limits

- Source preparation performs zero payload reads on the streaming path (explicit counter checked). Both paths incrementally hash the transmitted bytes; stream does not omit final integrity validation.
- /proc/self/io rchar/write_bytes and getrusage CPU/RSS describe the entire instrumented Linux process, not isolated hardware counters or power consumption. Warm source page cache; rchar counts logical read syscalls, not mmap or physical disk reads.
- Disk is the 10ms sampled sum of unique (device,inode) regular files in task-owned app temp plus output; source fixture excluded, tiny ticket/log files included, hard links counted once. Allocated blocks do not resolve shared reflink extents. Sampled peaks may miss short-lived duplicates and are not a hard upper bound.
- Export/commit is the legacy receive-completed event versus successful stream no-clobber commit. Legacy does not explicitly fsync; stream syncs its file before commit, neither promises parent-directory crash durability. API return includes legacy endpoint close; stream retains endpoint/connection until explicit shutdown and returns after its completion receipt transport acknowledgement. Do not attribute all API-return savings to transfer itself.
- Cold legacy baseline is the already optimized compatible engine, without app advisory metadata. Do not compare these numbers directly against the earlier app-orchestration benchmark.
- Repeated receive byte-comparison occurs after each individual receive timer; aggregate process metrics can include comparison/sampling overhead. No p95, crash durability, production relay auth, phone GUI, unknown-length Android pipe or internet SLO claim.
- Streaming is an explicit experimental single-regular-file API. No legacy ticket or installed client was automatically migrated; no retry/resume or disk spool is provided.
