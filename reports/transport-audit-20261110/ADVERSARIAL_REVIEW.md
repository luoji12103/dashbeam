# DashBeam 最新上游降延迟方案：独立对抗审查

日期：2026-11-10。范围：设计与源码只读复核，不实施、不编译、不使用真实配置或凭据、不运行 GUI、不修改生产。

状态：已完成独立基线、完整草案审查与修订复核。复核快照为 `UPSTREAM_DESIGN_DRAFT.md` 362 行，SHA-256 `82cf62cdb9e59df5473fe2d1bdd95832beef69832156e871911117cacb895b8f`。未发现需要推翻方向的 P0；两项 P1 和两项 P2 规范缺口已在草案中处置，没有未处置的 P0/P1 设计阻塞。结论是可进入隔离原型与实现验证，不是密码协议代码、持久性、手机路径或 3 秒 SLO 已验收。

## 1. 版本与证据边界

- 最新原作者隔离 checkout：`/Users/luoji12103/Codex/dashbeam-upstream-audit-20261110`，HEAD `b41eddb16b226667de6f8dd7f1326a0ce6bf5e7d`。本次核验其 `engine/Cargo.lock`、`src-tauri/Cargo.lock`、`wasm-bridge/Cargo.lock`，三者均为 `iroh / iroh-base / iroh-relay 1.3.0`、`iroh-blobs 0.103.1`。
- 本地定制版 `/Users/luoji12103/Codex/dashbeam-macos` 的 engine 与 Tauri lock 仍为 `iroh 1.0.3`、`iroh-blobs 0.103.0`。现有 DEV 实测是该旧栈的基线，不能证明上游 1.3.0 的性能或兼容性。
- `dev-relay.json` 是禁用 IP transport、核验两端所选 relay 的真实应用载荷测试；每个 relay 两个顺序 16 MiB stream，收到后 ACK，逐字节验证固定模式。没有磁盘持久性 ACK、blobs、UI 或手机 SAF 路径。第二个 stream 的 first-payload 时间仍从最初 connect 起算，不能解读为其自身 TTFB。
- `dev-share.jsonl` 的非零进度事件不是首个网络字节；engine payload duration 排除连接、sizes probe、导出。与最新上游数据不可混合成一个延迟分位数。
- Mac relay probe 只有 TLS/WebSocket/auth/Ping，既非 QUIC 文件传输也非手机端验收。两个不同位置/网络的 GZ/SH 排名不能互相覆盖。

## 2. 已核上游关键路径

以下源码定位相对于最新原作者 checkout；这些是现状，不是方案已经实现的行为。

| 现状 | 证据 | 设计含义 |
| --- | --- | --- |
| 元数据先完成总大小与缩略图，再开始 share | `src-tauri/src/commands.rs:238-273,320-346` | 缩略图可后台补齐；必要的发送集合、名称、大小语义与用户同意必须保留 |
| 原始路径全量导入后才运行 share session | `engine/native/src/send.rs:89-108`；`engine/native/src/import.rs:39-92` | 旧 BlobTicket 根 hash 未就绪前不能拿到可用旧票据；仅把 await 放后台不能实现先发 |
| Ticket 显式含 collection 根 hash | `engine/protocol/src/send.rs:211-216` | 无全文件预哈希路径必须有新版本会话/票据，不能假装旧客户端兼容 |
| `run_share_on_endpoint` 存在，但主命令没有调用它 | `engine/protocol/src/send.rs:232-275`；`src-tauri/src/commands.rs:273` | helper 存在不等于数据链路已经常驻复用 |
| 常驻 node router 当前仅注册 CONTROL_ALPN | `engine/native/src/node.rs:2247-2277` | 新数据 ALPN 需要真正注册、路由、生命周期与权限检查 |
| node 配对 hook 对非 CONTROL_ALPN 直接放行 | `engine/native/src/node.rs:74-82` | 不能把“同一 Endpoint”当成数据层自动继承配对权限 |
| metadata 用临时 Endpoint，成功后关闭；download 重新 bind | `engine/protocol/src/receive.rs:268-313,386`；`engine/native/src/receive.rs:67-79` | 控制通道在线、metadata 成功都不证明 payload QUIC 连接可复用 |
| metadata 请求先写一个 marker，再读响应 | `engine/protocol/src/receive.rs:354-365`；`engine/protocol/src/send.rs:49-65` | 可在新版 OPEN 首帧合并语义；不能删除首写后让双方都等待读取 |
| payload 测时晚于 connect 与 sizes probe | `engine/protocol/src/receive.rs:137-180` | 新的端到端 SLO 不能继续只采旧 download duration |
| Android URI 先复制到本地 cache | `CopyUtils.kt:104-147`；`NativeUtils.kt:529-575` | 新增 streaming source 需独立处理不可 seek、未知长度、权限与进程死亡 |
| exporter 验证路径组件与冲突名，再复制 | `engine/native/src/export.rs:28-65,103-120` | 新 streaming exporter 不能绕过这些行为；还应审查 symlink/TOCTOU 与未完成文件暴露 |

官方版本源码复核：

- [iroh-blobs v0.103.1 ImportMode](https://github.com/n0-computer/iroh-blobs/blob/v0.103.1/src/api/proto.rs#L630)：`TryReference` 明确假定导入后源不变；`Copy` 才是 safe default。stat 前后相同并不能证明点时快照。
- [iroh v1.3.0 relay clients](https://github.com/n0-computer/iroh/blob/v1.3.0/iroh-relay/src/server/clients.rs#L73)：同 EndpointId 新连接替代 active，旧连接收到 `SameEndpointIdConnected` 并转 inactive；旧 active 退出时可能重新提升旧连接。因此必须避免同一长期身份并存两个独立 Endpoint。
- [iroh v1.3.0 0-RTT 文档](https://github.com/n0-computer/iroh/blob/v1.3.0/iroh/src/endpoint/connection.rs#L522)：0-RTT 可重放；0.5-RTT 可能早于客户端身份认证。第一轮优化应优先复用已认证会话，不把状态变更或受授权保护的 payload 放进未经审查的 early data。
- [Noq-proto 1.3.0 的实际发布源码](https://github.com/n0-computer/noq/blob/c1f411562e6078852749b8bcf1190523096a107f/noq-proto/src/config/transport.rs#L548)：从官方 crates 包内读取，包 SHA-256 `7c1e5b6fe668491eca022f745a0a9402585626c73a7b839b3424ace15d6a9c8f` 与最新上游 lock 相同；默认单 stream 接收窗 `1,250,000 B`，连接接收窗 `VarInt::MAX`，发送窗 `10,000,000 B`，默认 `Cubic`。不是照搬 1.1.0 缓存。
- [Iroh 1.3.0 QUIC wrapper](https://github.com/n0-computer/iroh/blob/v1.3.0/iroh/src/endpoint/quic.rs#L140)：官方包 SHA-256 `885787b892b5e2507c701f132ecbd45d2bad4bbb75157a19427087c16dadb833` 与 lock 相同；`QuicTransportConfigBuilder::new` 只覆盖 keepalive、multipath、NAT traversal、handshake migration，没有覆盖上述窗口或 Cubic。最新 DashBeam 产品源码未见 transport config 自定义。窗口/RTT 仅是候选瓶颈分析，不证明本次低速根因。

## 3. 不可妥协条件与反例

严重级别：P0 为可导致未授权内容披露、密码学破坏或已承诺可退出却不可恢复的数据丢失；P1 为阻塞协议安全/正确性验收的设计缺口；P2 为性能、可维护性或测量口径缺陷。这里只列检查条件，不预先认定草案违反。

| ID | 条件 | 必测反例与验收 |
| --- | --- | --- |
| I01 | 把 UI Ready、SessionConnected、FirstAuthenticatedPayload、FinalVerified、ExportCommitted 分开 | 缩略图先出现而 payload 未到不得计成功；空文件单独计完成，不伪造非零首 payload |
| I02 | 同一 identity 只有一个拥有者 Endpoint；跨 ALPN 只能复用 Endpoint，不假定复用已建 QUIC Connection | 同时开启 control、metadata、send、receive；网络重建时不得生成同身份并行 endpoint 或 relay active 抖动 |
| I03 | 每个 OPEN/RESUME/GET/ACK 按 transfer、recipient、epoch 与能力检查 | 未配对客户端连新增 ALPN、旧 grant 用于新文件、已撤销 grant 重放，均不得读取数据或释放缓存 |
| I04 | 新流协议绕开预哈希时，完整性承诺必须明确分阶段 | 截掉最后一帧、丢失 EOF、把末尾空文件省略、重排文件、缩短 manifest 长度，均不得 FinalVerified；首帧 AEAD 成功只证明局部认证 |
| I05 | 选定文件集合、文件身份、顺序/路径与最终 manifest 必须绑定 | Final manifest 不能悄悄变成实际成功读取的子集；未知大小需显式 unknown，必要时用户同意后补总量 |
| I06 | 源版本与恢复能力诚实；发出的密文不可按可变源重新构造 | 同大小内容改写/改回 mtime、inode 替换、文件被截断、SAF 不可 seek、URI 权限跨重启丢失；必须使用稳定快照或明确失败/恢复能力边界 |
| I07 | AEAD nonce 对每个 key 唯一，含文件和帧索引的域分离 | 两文件都从 frame 0 开始、resume 后 counter 归零、同 transfer 重新选文件、manifest 与 data 共用 nonce 均不能复用 key/nonce；AAD 不会弥补 nonce 重用 |
| I08 | direct 与 spool 共用不可变 frame 身份及去重账本 | 同帧双路径到达只落盘/计进度一次；同身份不同 digest 必须拒绝；重传应重用同一已生成密文，不按变化源同 nonce 重加密 |
| I09 | ACK 层级明确：接收/认证不等于持久可恢复 | 在每个写数据、sync、journal commit、发 ACK 之间 crash；恢复不能失去任何已 durable ACK 的范围；接收端 ACK 需要端到端认证，服务器不得伪造 |
| I10 | 发送端退出条件是剩余全部数据与终结 manifest 的可恢复覆盖 | 20% cache 满、接收方离线、未发送后缀仍在源上时禁止“可退出”；多接收者按每个承诺对象分别满足覆盖与租期 |
| I11 | 硬盘缓存上限 `C = min(0.20 × F, 1 GB)`，不是启动水位也不是 RAM 分配 | 第一帧即可转发；4 MiB 是最大落盘段，不是最小开流量；小于一帧的 C 必须缩帧/按规则无缓存直转/明确拒绝，不可越限 |
| I12 | per-transfer 上限之外还有原子全局磁盘配额、系统保留量与公平性 | 多个发送方同时 admission、磁盘被其他进程耗尽、journal/密文/tag/临时文件占用均计账；未知 F 不得通过谎报大小突破 quota |
| I13 | 两跳流控是独立应用 spool 协议，不是缓存/重放 QUIC 包 | client→server 与 server→receiver 独立连通、窗口、背压和恢复；磁盘满时停止接收 credit，内存不能继续无界增长 |
| I14 | 回收与撤销必须有恢复后仍有效的单调状态 | 延迟 ACK、TTL 同时到期、revoke 与 resume/commit 并发、服务器重启；旧 ACK 不能复活 transfer，删除后的密文不能重新标成 durable |
| I15 | 多接收者回收不能由最快者单独触发 | A 收完、B 离线；只能按明确受承诺接收者集合/租期释放，或向 B 明确告知承诺终止；撤销不能保证抹除已下载的明文 |
| I16 | 继续保留用户同意、安全文件处理与 metadata 解析边界 | 未同意接收不得自动发布/打开数据；未完成文件保留隔离状态；路径穿越、绝对路径、symlink、超长 metadata、超大长度和整数溢出均拒绝 |
| I17 | 预热与后台策略必须受移动系统和功耗约束 | Doze、屏幕锁定、网络切换、进程被杀、前台服务停止；不得假定常驻可靠或为了低延迟无限保活所有配对设备 |
| I18 | SLO 包含可复现起点、人工交互分拆和全部失败样本 | SendIntent→首认证 payload 与 Accept→首认证 payload 分开；cold/warm、direct/relay、大小/设备分层；超时/失败计分母，不用旧成功样本掩盖回退/超时 |

跨设备测时补充：双方单调时钟没有共同 epoch，不能直接相减；`transfer_id` 只能关联事件。可由发送端单调计时到接收端“首帧认证成功”的回执，得到包含回程的保守上界，或使用受控同步与误差区间。上界低于 SLO 可支持单程达标，上界超标不能独自证明单程超标。接收方已缓存的重复帧不能反复触发首帧事件或进入新传输成功分母。

## 4. 可接受的最小方向

- 将缩略图、丰富预览从关键路径移出，保留最小安全元数据和授权流程。
- 活跃设备复用常驻 Endpoint 与同一新版会话 ALPN 中已认证的 QUIC 连接；保留隔离的 legacy Blob 路径作为显式兼容回退。
- 新协议采用有界认证小帧，可边读、边认证、边转发；最终由受认证 manifest 承诺完整集合和文件摘要。不能把“局部帧已认证”显示为“完整文件已验证”。
- 可选磁盘 spool 保存 E2EE 密文，64 KiB 是可调帧上限、4 MiB 是可调段上限，flush 时限和 durable ACK 周期由实测确定。首包转发与落盘批量 ACK 分离。
- 无完整离线副本时坚持剩余范围覆盖条件。小缓存能吸收速率差并降低发送方等待，但不能把大文件变成离线永久托管。

## 5. 草案实审发现与处置

已完整阅读首稿，并核对下表修订后的正文。下列“已处置”仅指设计契约已明确；故障场景是实现阶段的验收要求，本审查没有虚构已运行这些测试。

| ID/级别 | 原设计缺口与违反场景 | 最小修订与本次复核 | 实现验收 |
| --- | --- | --- | --- |
| R1 / P1，已处置 | FINAL_MANIFEST 允许排除/失败清单，却未清楚规定 Partial 与 Completed 的关系。选中目录含两个文件，其中一个不可读，若只给剩下一个文件生成有效 manifest 并完整完成，会把选择集合悄悄缩小，违反 I04/I05 | 草案 `4.2` 第 114 行、状态机第 139–140 行、FINAL_RECEIPT 第 160 行：原选择范围内枚举/读取失败不可产生完整 verified receipt；Partial 是原 offer 的终结状态。接受子集需用户确认并生成新 offer/transfer_id | 让目录中一个文件在枚举/读取时失败：保留已验证文件但不得完整 Completed。确认子集后只允许新 transfer 完成；空目录必须和枚举失败区分 |
| R2 / P1，已处置 | 多接收者拆成独立 transfer 后，各自拿 C(F)，可能让同一次 10 GB 分享给三人消耗 3 GB，绕开用户规定的 1 GB 上限，违反 I11/I12 | 第 152、200、218 行增加 share_id 父预算；所有 child 的 D+R 合计仍不超过同一逻辑集合的 C(F)，各 child 保持独立密钥/密文/ACK。首版新流协议只开放单接收者，原广播走显式 legacy；扩展多接收者前必须实现父预算 | 同一 share 的并发 child 准入/credit 更新必须原子计入父预算，不能因新 child、重连或取消一个 child 重复释放额度；首版多接收者请求必须明确走 legacy 或拒绝新路径 |
| R3 / P2，已处置 | “源可及时首读”等前提可能在失败后被当成剔除样本的理由，仅报告成功热样本的 p95，不能证明用户等待改善，违反 I18 | 第 77、281、308 行：预先冻结 eligible 矩阵；所有 eligible 失败/timeout 入分母；同时报告全程 SendIntent→首认证回执、Accept 后阶段、选目标/同意等待、冷启动、legacy 回退数量；跨设备采用同端回执上界或同步误差区间；小样本不声称可靠 p95 | 故意加入超时、回退、人工等待样本，核对统计分母不减少，阶段和全程可对应；三次样本只给逐次值/范围，不生成可靠分位数结论 |
| R4 / P2，已处置 | “Cancel/Delete 立即撤销”未区分本地与不可达的远端。P 断网时本地取消不可能立刻证明远端已删除，且迟到 ACK/续租可能与终结竞态，违反 I14 | 第 269 行限定本地立即，远端须确认或租约到期，P 不可达显示远程清理待确认；持久终结后迟到 APPEND/ACK/RENEW/COMMIT 不得复活 | 隔断 P 后取消：本地停止读取，界面不报告远端已删除。P 处理终结并重启后注入旧写入/ACK/续租，任务保持终结，实际磁盘回收量准确 |

## 6. 通过的设计边界与待验证事项

本次审查认可以下方向：旧 BlobTicket 根 hash 约束用明确新版本协议解决；同 identity 单一 Endpoint owner 且按 transfer/recipient 校验新 ALPN 权限；必要 metadata、既有自动接收授权和首写 OPEN 保留；64 KiB 帧认证与最终集合签名分开；per-recipient key 与 nonce 域分离；不确定恢复映射时终止旧 epoch；源快照与 CapturedStream 明确区分；direct/spool 按不可变帧去重；磁盘 cap 不等同 RAM、不作为启动水位；持久 ACK 与最终目标提交分层；首版发送方等待接收端最终 receipt，不宣传仅有部分服务器缓存时可退出。

草案还明确了小文件/未知 F 的无 spool 路径、未同意 Offer 仅在线转发而不建立磁盘信箱、全局/设备/父 share 配额、底层 QUIC/H2 内存预算、TTL/tombstone、SAF 不可 seek 与跨重启权限限制、Doze/后台及功耗边界，避免用可用性标记或微基准替代手机路径验收。

仍需实施阶段完成，而非本次设计交付阻塞：冻结 canonical 编码/KDF/manifest 算法与跨语言测试向量；独立密码代码复核；ACK 前后 crash、ENOSPC、撤销竞态的故障注入；实际分配块含 WAL/目录/compaction 的峰值计账；真实移动设备和目的 provider 的导出与后台测试。窗口、flush 周期、预热超时等是可调实验参数，不能因为尚未调优而无限延期交付设计，也不能提前承诺性能。

对开发原型的证据要求已同步给原型负责人：必须记录实际 iroh 1.3.0、profile、冷热、路径与失败样本；同一进程共享 Instant 可以直接比较两端时间，跨进程/设备则不能；raw QUIC 首 payload 不等于应用 AEAD 首帧，磁盘帧写入测试不等于全 WAL/崩溃恢复。这一审查不替原型负责人或主负责人认定实验结果。
