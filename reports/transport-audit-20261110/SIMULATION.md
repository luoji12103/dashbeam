# 真实流式请求实验

本实验为最新上游 `b41eddb16b226667de6f8dd7f1326a0ce6bf5e7d` 的新增协议假设提供隔离证据。最终版 **32/32 次传输通过**：24 次同机直连，8 次强制上海自托管 relay。64 MiB 冷 relay 首实际文件字节约 **0.90 秒**，复用连接约 **0.15–0.16 秒**；完整文件约需 14–15 秒。首载荷不必等完整文件准备/传输结束。

运行的是精确 **Iroh 1.3.0** 的真实 Endpoint、QUIC 连接与双向流，读取真实合成磁盘文件并写入接收文件；没有用 sleep 模拟网络请求。它使用新增实验 ALPN，不使用 BlobTicket，也没有证明现有 BlobTicket 能在预哈希之前开始传输。

## 可复现源码与协议

- [streaming_request_probe.rs](streaming_request_probe.rs)：完整独立原型。
- [streaming-request-Cargo.toml](streaming-request-Cargo.toml)：依赖与编译 profile；`dev`、`opt-level=0`、`debug=0`、禁增量，2 个编译 job、2 个 Tokio worker。
- [streaming-request-Cargo.lock](streaming-request-Cargo.lock)：完整实际解析锁文件。DEV rustc `1.96.0`、x86_64；首次离线构建 67 秒，追加首帧参数后增量构建 1.89 秒。
- DEV 独立目录 `/home/luoji/code/dashbeam-streaming-request-20261110`；没有覆盖 `/home/luoji/code/dashbeam-macos` 的脏源码、锁文件或 target。本机没有编译产品。

双方各生成一个随机临时密钥，建立连接后同时核验 `remote_id` 等于本次预期对端；不复用产品身份、配对或 ticket。冷场景从 SendIntent 开始创建双方 Endpoint、等待在线、建连；热场景在同一对 Endpoint 和同一 QUIC Connection 上新建 stream。

```text
SendIntent
  → OPEN {version, transfer_id, source_size, frame_max=65536, first_frame_bytes}
  ← ACCEPT {transfer_id}                  自动同意，不含人工等待
  → [u32长度][最多64KiB实际文件内容] …     边读盘、边增量BLAKE3、连续发送
  ← FIRST_FRAME {transfer_id, bytes}      第一完整帧已收、更新hash、write返回
  → 0长度 + MANIFEST {transfer_id, bytes, blake3} + EOF
  ← COMPLETE {bytes, blake3, verified} + EOF
```

首帧取 64 KiB 或 8 KiB，双方严格核验 OPEN 声明、首帧长度和 ACK；后续仍为最多 64 KiB。首帧回执与后续发送并发，发送者不为等待首帧回执暂停后续数据。OPEN 不含内容摘要；完整摘要由发送时实际读到的字节产生。接收方核验真实读取总长、末尾 manifest、增量摘要、预设 fixture 摘要及 EOF；最终 ACK 后另外重读接收文件核验长度/摘要，并单独记录这段耗时。

## 计时定义

- `first_actual_file_byte_from_shared_send_intent_ms`：接收方读到首个实际文件字节；排除 OPEN、JSON 与长度 header。本实验两端在同一进程，使用共享 `Instant`，此值可直接比较。
- `first_frame_ack_from_send_intent_ms`：发送方时钟从 SendIntent 到收到完整首帧回执。包含回程，因此可作为跨设备场景的保守上界；本实验没有相减不同设备时钟。
- `complete_ack_from_send_intent_ms`：发送方收到最终长度/摘要通过的 ACK。接收输出的独立重读不计入此值。
- `file_open_stat_ms`、`source_first_read_ms`、`source_read_total_ms`：发送端局部读盘阶段；读取与网络反压交错，不能将局部阶段总和当另一种端到端耗时。
- 首字节由 QUIC 的身份认证与加密保护；**没有实现拟议的应用 AEAD 帧验证**。FIRST_FRAME 表示完整帧经 QUIC 接收并更新增量 hash，不能解释为已完成全文件摘要校验。

冷/热均使用同一台 Debian DEV 上的两个 Endpoint。`direct` 禁 relay；`relay` 禁 IP transport，通过两端 path watcher 核验所选 relay。两端物理上均在 DEV，不能代表 Mac↔手机、不同运营商、Android 后台或全球网络成绩。

## 测量结果

原始记录：[直连64KiB](streaming-direct-64k.jsonl)、[直连8KiB](streaming-direct-8k.jsonl)、[上海relay64KiB](streaming-relay-first-65536.jsonl)、[上海relay8KiB](streaming-relay-first-8192.jsonl)；派生统计 [streaming-summary.json](streaming-summary.json)。直连在 2026-11-10 09:41 UTC 完成；relay 在同日约 09:42–09:43 UTC 完成。各文件保留 run metadata、fixture、每次 transfer 和 cleanup。

下表单位均为 ms。直连每格 n=3，显示中位数及首字节范围；冷包含新建双方 Endpoint。relay 每格 **n=1，仅单次观察，不是中位数或分位数**。

| 路径 | 首帧 | 文件 | 冷首实际字节 | 热首实际字节 | 冷首帧ACK | 热首帧ACK | 冷/热完成ACK |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 同机直连 | 64KiB | 1MiB | 55.91 [47.07–86.16] | 3.00 [2.45–3.74] | 64.64 | 4.06 | 76.37 / 24.56 |
| 同机直连 | 64KiB | 64MiB | 56.58 [46.52–65.52] | 14.99 [14.64–16.64] | 61.73 | 17.01 | 1195.84 / 1368.53 |
| 同机直连 | 8KiB | 1MiB | 58.09 [51.54–100.36] | 2.19 [1.98–3.05] | 58.84 | 3.39 | 74.41 / 19.13 |
| 同机直连 | 8KiB | 64MiB | 53.01 [51.83–53.84] | 17.26 [15.03–20.78] | 54.60 | 17.72 | 1183.75 / 1175.65 |
| 强制上海relay | 64KiB | 1MiB | 1028.28 | 147.77 | 1334.85 | 227.67 | 1808.21 / 422.55 |
| 强制上海relay | 64KiB | 64MiB | 896.95 | 161.70 | 1205.64 | 283.85 | 15451.12 / 14476.86 |
| 强制上海relay | 8KiB | 1MiB | 934.11 | 157.59 | 1028.78 | 263.30 | 1887.38 / 683.23 |
| 强制上海relay | 8KiB | 64MiB | 895.07 | 150.79 | 994.87 | 258.86 | 15485.01 / 13855.08 |

本轮没有传输失败/超时；最终版计划 32、成功 32，16 对 Endpoint 清理均成功。首轮参数化前另有 [12/12 直连验证](streaming-direct-initial-64k.jsonl)，仅保留辅助记录，不混入最终分母或上表。所有最终样本均核验双方期望身份和实际所选 IP/relay 路径、源/接收字节数、末 manifest/摘要、EOF，并通过接收文件独立重读。

64 KiB relay 的两个冷样本，Endpoint pair bind 为 99/156 ms，等待双方 online 为 514/547 ms，connect 为 154/165 ms；热连接省去这些阶段。1 MiB 与 64 MiB 的冷首包相近，说明该协议没有总文件预哈希屏障。热大文件开头仍受先前大流的 QUIC 状态、调度及反压影响，不能宣称首包时延严格与文件大小完全无关。

发送端 open/stat 全样本约 0.05–0.80 ms，首读约 0.04–0.16 ms；这些是刚写入页缓存的 DEV 局部成绩。8 KiB 首帧降低了本轮冷 relay 完整首帧 ACK，但热 1 MiB 样本方向相反；两档按顺序跑，relay 每格只测一次，不能归因为帧大小，也不能宣称 8 KiB 是全局最优。它适合作为后续交错 A/B 的试点。更小首帧不等于已提升持续吞吐，未调大窗口或拥塞控制。

合成文件在计时前刚写入并 fsync，fixture 期望摘要在生成时计算；这不是冷盘。传输路径自身不预读完整源文件来得到摘要。3 次短样本不报告 p95；本轮不再扩大到 GZ 或压力测试。最终版文件载荷为直连 780 MiB 加 relay 260 MiB，共 1040 MiB；另有初轮直连 390 MiB，累计 1430 MiB，不含协议开销或磁盘重读。

## 验收边界

这里验证请求/连接复用和无全量预哈希流式首包的可行性。没有完整 UI、系统分享入口、Android SAF/provider、权限、应用 AEAD、生产授权、spool、持久 ACK、崩溃恢复或电耗验收。接收方只 `flush`，没有 `fsync`，最终 ACK 不表示断电后可靠交付，也不表示发送方现在可以退出。

编译前 Cargo 自带 HTTP 下载两次遇 sparse index 低速超时；同机 curl 可读相同公共 URL。实验用 curl 补入缺失公共 registry 缓存，并以最新上游锁文件中的 SHA256 校验 `.crate` 后离线编译。没有改系统/生产代理，没有更换 Iroh 版本，没有将编译失败当作网络性能样本。

源码 SHA256 `4da590f3e9f9ad480ce0f358949d95e0218604140a39d84a3f347719d657a72a`；锁文件 SHA256 `4a83fc9f9f7c872246c970dd0ffaf10b56175bbf3282bc6c64513d1e6139e327`。DEV 实验目录约 654 MiB，结束时根盘约 25 GiB 可用。无常驻 probe 服务；fixture 和接收临时文件由程序完成后移除。relay token 由主负责人从本机有效设置读取，仅经加密 SSH stdin 传入进程内存，没有进入参数、日志或报告。
