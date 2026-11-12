# Source-stream engine A/B results

72/72 process-pair samples successful; 72/72 expected-path verified; 96 file receives recorded.

Medians below. Payload is an internal positive progress / first stream-frame proxy, NOT a packet-capture first byte. First-transfer end-to-end includes cold share preparation and 10ms ticket polling. No GUI, metadata advisory, device consent or Android provider staging is simulated.

| Case | Version | Valid / attempted | Share ready (s) | Share → payload (s) | Share → export/commit (s) | Share → API return (s) | Sender prepare rchar (MiB) | Sampled app+output peak disk (MiB) |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1. cold-direct-small-lan | legacy | 3/3 | 0.038 | 0.088 | 0.105 | 0.106 | 1.00 | 2.67 |
| 1. cold-direct-small-lan | stream | 3/3 | 0.027 | 0.073 | 0.093 | 0.122 | 0.00 | 1.00 |
| 2. cold-direct-large-lan | legacy | 3/3 | 0.160 | 0.227 | 4.771 | 4.771 | 256.00 | 509.84 |
| 2. cold-direct-large-lan | stream | 3/3 | 0.031 | 0.085 | 2.456 | 2.484 | 0.00 | 256.00 |
| 3. cold-direct-small-wan | legacy | 3/3 | 0.024 | 0.445 | 0.928 | 1.007 | 1.00 | 4.02 |
| 3. cold-direct-small-wan | stream | 3/3 | 0.023 | 0.238 | 0.818 | 0.925 | 0.00 | 1.00 |
| 4. cold-direct-large-wan | legacy | 3/3 | 0.073 | 0.474 | 12.156 | 12.201 | 64.00 | 130.55 |
| 4. cold-direct-large-wan | stream | 3/3 | 0.030 | 0.246 | 11.982 | 12.062 | 0.00 | 64.00 |
| 5. cold-relay-small-lan | legacy | 3/3 | 0.286 | 0.348 | 0.377 | 0.379 | 1.00 | 3.02 |
| 5. cold-relay-small-lan | stream | 3/3 | 0.289 | 0.358 | 0.381 | 0.410 | 0.00 | 1.00 |
| 6. cold-relay-small-wan | legacy | 3/3 | 0.561 | 1.132 | 1.606 | 1.686 | 1.00 | 4.02 |
| 6. cold-relay-small-wan | stream | 3/3 | 0.562 | 0.917 | 1.489 | 1.596 | 0.00 | 1.00 |
| 7. cold-relay-large-wan | legacy | 3/3 | 0.557 | 1.119 | 13.127 | 13.168 | 64.00 | 130.55 |
| 7. cold-relay-large-wan | stream | 3/3 | 0.552 | 0.917 | 12.943 | 13.036 | 0.00 | 64.00 |
| 8. cold-relay-lossy | legacy | 3/3 | 0.814 | 1.841 | 32.819 | 32.959 | 16.00 | 34.15 |
| 8. cold-relay-lossy | stream | 3/3 | 0.794 | 1.516 | 30.911 | 31.062 | 0.00 | 16.00 |
| 9. repeat-relay-small-wan | legacy | 3/3 | 0.560 | 1.122 | 1.603 | 1.682 | 1.00 | 4.02 |
| 9. repeat-relay-small-wan | stream | 3/3 | 0.572 | 0.934 | 1.514 | 1.611 | 0.00 | 1.00 |
| 10. repeat-direct-small-wan | legacy | 3/3 | 0.038 | 0.446 | 0.926 | 1.005 | 1.00 | 4.02 |
| 10. repeat-direct-small-wan | stream | 3/3 | 0.029 | 0.252 | 0.833 | 0.938 | 0.00 | 1.00 |
| 11. high-bdp-direct | legacy | 3/3 | 0.070 | 0.470 | 5.699 | 5.742 | 64.00 | 130.55 |
| 11. high-bdp-direct | stream | 3/3 | 0.028 | 0.253 | 1.797 | 1.904 | 0.00 | 64.00 |
| 12. high-bdp-relay | legacy | 3/3 | 0.558 | 1.111 | 6.378 | 6.423 | 64.00 | 130.55 |
| 12. high-bdp-relay | stream | 3/3 | 0.556 | 0.930 | 2.660 | 2.752 | 0.00 | 64.00 |

## Repeated receives

Both versions reuse the same sender share. Stream also keeps its receiver endpoint/connection; legacy download creates a new receiver per call. Each process sample contributes the median of its two repeat receives. These are not new cold user-share measurements.

| Case | Version | Repeat receive → payload (s) | Repeat receive → return (s) |
| --- | --- | ---: | ---: |
| 9 | legacy | 0.528 | 1.086 |
| 9 | stream | 0.085 | 0.372 |
| 10 | legacy | 0.380 | 0.934 |
| 10 | stream | 0.083 | 0.356 |

## Resource and interpretation limits

- Source preparation performs zero payload reads on the streaming path (explicit counter checked). Both paths incrementally hash the transmitted bytes; stream does not omit final integrity validation.
- /proc/self/io rchar/write_bytes and getrusage CPU/RSS describe the entire instrumented Linux process, not isolated hardware counters or power consumption. Warm source page cache; rchar counts logical read syscalls, not mmap or physical disk reads.
- Disk is the 10ms sampled sum of unique (device,inode) regular files in task-owned app temp plus output; source fixture excluded, tiny ticket/log files included, hard links counted once. Allocated blocks do not resolve shared reflink extents. Sampled peaks may miss short-lived duplicates and are not a hard upper bound.
- Export/commit is the legacy receive-completed event versus successful stream no-clobber commit. Legacy does not explicitly fsync; stream syncs its file before commit, neither promises parent-directory crash durability. API return includes legacy endpoint close; stream retains endpoint/connection until explicit shutdown and returns after its completion receipt transport acknowledgement. Do not attribute all API-return savings to transfer itself.
- Cold legacy baseline is the already optimized compatible engine, without app advisory metadata. Do not compare these numbers directly against the earlier app-orchestration benchmark.
- Repeated receive byte-comparison occurs after each individual receive timer; aggregate process metrics can include comparison/sampling overhead. No p95, crash durability, production relay auth, phone GUI, unknown-length Android pipe or internet SLO claim.
- Streaming is an explicit experimental single-regular-file API. No legacy ticket or installed client was automatically migrated; no retry/resume or disk spool is provided.
