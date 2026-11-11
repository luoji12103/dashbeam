# DashBeam 最新上游低时延传输设计草案

日期：2026-11-10。交付性质：源码核验后的设计建议，未实施、未编译、未部署，未接触凭据或生产配置。本稿提供给主报告整合及独立对抗审查；性能数字若没有注明“实测”，均为设计预算或计算值。

## 1. 推荐结论

推荐保留 iroh 负责设备身份绑定、QUIC、地址发现、穿透和普通中继；新增一个**可复用的流式会话 ALPN**，将 Offer、Accept、最小元数据和文件流放在同一已认证会话内。发送动作先创建传输记录和展示目标，选择目标与源文件打开并行；不再等待整文件 hash、全目录扫描、缩略图或临时中继探测才允许邀请。采用最大 64 KiB 的独立认证帧边读边传，试点首帧先发真实的 8 KiB，完整文件/集合承诺在最后认证。

服务器增加独立的**应用层端到端密文 spool**，作为受限的两跳传输路径。它的硬盘占用上限是 C(F) = min(floor(0.2 × F), 1,000,000,000 bytes)，并计入段、日志、索引、临时文件及文件系统分配开销。第一帧就可转发；20% 是积压上限，不是开传门槛。默认快速路径仍是现有 iroh 对等连接，只有冷连接迟迟没有可用数据路径、接收端短时掉线或有明确缓冲收益时才启用 spool。

不建议第一步重写 NAT 穿透、替换 QUIC 栈、提高服务器限速、无差别增大所有窗口、增加重试层数或令所有传输先经过服务器落盘。先删掉可证实的串行准备工作，再增加真正支持边读边传的协议；spool 是容错和速率解耦能力，不能保证比一条良好的直连更快。

3 秒仅作为**已接受、源可及时读取、目的地可写、双方前台/后台服务可运行、网络在规定范围内**的首个已认证明文帧目标。不是任意文件、任意网络或冷启动的承诺；旧版 Blobs 降级不进入这个 SLO。

## 2. 锁定基线与实际上游差异

源码根目录：[dashbeam-upstream-audit-20261110](/Users/luoji12103/Codex/dashbeam-upstream-audit-20261110)。本稿上游行号均指 HEAD b41eddb16b226667de6f8dd7f1326a0ce6bf5e7d，main，提交时间 2026-10-19T18:58:00+01:00。Git remote 为作者仓库 [tonyantony300/dashbeam](https://github.com/tonyantony300/dashbeam)。

本次只读查询官方 releases/latest：最新正式发布 [v0.7.2](https://github.com/tonyantony300/dashbeam/releases/tag/v0.7.2)，2026-10-14T16:11:15Z，target_commitish = 9f2dfaf99caee335a5dd9979e0baba24fd2d9c50。**main 源码与该 release 目标不是同一提交**，不能把 main 的设计分析表述为已安装 v0.7.2 二进制实测。

实际依赖已同时核对 manifest 与两个 lockfile：

| 项目 | manifest | engine/Cargo.lock | src-tauri/Cargo.lock |
| --- | --- | --- | --- |
| iroh | engine/native/Cargo.toml:20，protocol/Cargo.toml:27，src-tauri/Cargo.toml:34：1.3.0 | 1680–1683：1.3.0 | 3455–3458：1.3.0 |
| iroh-blobs | native/Cargo.toml:19，protocol/Cargo.toml:26，src-tauri/Cargo.toml:35：0.103.1 | 1749–1752：0.103.1 | 3524–3527：0.103.1 |
| iroh-relay | 间接依赖 | 1873–1876：1.3.0 | 3648–3651：1.3.0 |
| iroh-base | 间接依赖 | 1730–1733：1.3.0 | 3505–3508：1.3.0 |
| QUIC 内核 | iroh 1.3.0 使用 noq | 2503 起：noq 1.3.0 | 需按实际依赖树归属；不能拿另外一项 quinn 0.11.11 代替 iroh 内核 |

当前定制版另在 /Users/luoji12103/Codex/dashbeam-macos，HEAD f3253bbbf6c1028821e2a66602ede9557cff4113，主负责人确认安装 build 70104。本稿没有将该定制版的行为当作新版上游事实。

| 环节 | 当前上游已确认的实现与行号 | 与旧定制版/推荐方案的关系 |
| --- | --- | --- |
| 发送准备 | commands.rs:237–274 先 build_send_metadata，再 resolve_relay，再 start_share_items；320–384 先总大小再缩略图 | 整体串行瓶颈仍存在；可先改变调用顺序 |
| 目录/多文件 | commands.rs:1223–1244 全目录统计；1266–1304 每项再次统计并依次生成缩略图；native/import.rs:160–188 再枚举 | 去除缩略图仍不足以保证海量目录快速就绪；扫描与数据流要分离 |
| Blob 入口 | native/send.rs:40–53 新建临时身份/端点；89–106 bind、store、全部 import；protocol/send.rs:198–216 等 online 后生成根 HashSeq 票据 | 先完整导入才能产出根 hash，是协议入口约束，不是仅前端显示慢 |
| import 是否整文件复制 | native/import.rs:43–57 使用 TryReference，等待 Done；67–90 汇总所有条目才存集合 | 不能把它一概写成整文件复制；但仍须完成内容导入/hash，且引用源必须稳定 |
| 常驻节点 | native/node.rs:2247–2277 只注册 CONTROL_ALPN；native/paired_connections.rs:31–43、319–331 维护长期控制连接 | “有在线列表”不等于文件端点或文件 QUIC 会话已预热 |
| 复用 helper | protocol/send.rs:232–276 存在 run_share_on_endpoint；全库查询只有定义和 lib.rs:45 导出 | 当前主路径未调用，不能写成已具备端点复用 |
| 中继探测 | protocol/relay.rs:172–192 临时端点；218–243 custom 分支每次外层 15 秒探测 | send、receive、独立 preview 都调用 resolve；把健康状态移到真正使用的常驻端点 |
| 接收 | commands.rs:449–567 resolve 后直接 download；native/receive.rs:67–79 再建接收端点、store；protocol/receive.rs:137–180 connect、hash_seq/sizes、execute_get | **上游没有定制版 receive 前 3 秒 metadata fetch，也没有其 10 秒 × 3 connect 与外层 × 3 download 包装** |
| 独立预览 | commands.rs:389–407 单独 resolve；protocol/receive.rs:260–426 临时端点，3 次尝试；每次 15 秒 connect、20 秒 open_bi、10 秒 write、20 秒 read；frontend/useReceiver.ts:277–319 再有 300 ms debounce | preview 不是接收必经步骤，但可能与接收产生重复工作；不把各阶段超时上限当常见耗时 |
| 邀请 | frontend/useSender.ts:655–679 等 send_items 返回才设置 ticket；812–849 要有 ticket 才邀请，并重新取大小；node.rs:1335–1382 最多等缓存连接 30 秒后再尝试 8 秒 connect | 选设备、探连接、读首帧应提早并行；online 不能成为唯一的禁用依据 |
| 导出 | native/export.rs:47–54 明确 ExportMode::Copy；native/receive.rs:81–102 在全量下载后导出；commands.rs:605–627、667–681 Android 再导出 SAF/MediaStore | 应减少下载后复制，但“已验证”“已落到目标”“可打开”仍是不同状态 |
| Android 发文件 | NativeUtils.kt:467–476 调 startUriCopy；529–575 先复制；CopyUtils.kt:121–147 整个 URI 读完才 completed；164–165 目录先全量枚举 | 仅修改 Rust hash 不会消除 Android SAF 前置全量复制 |
| Android 保活 | PresenceService.kt:63–72 connectedDevice 前台服务，99–129 multicast/Wi-Fi low-latency lock；src-tauri/src/presence_service.rs:34–51 基于配对数/可发现状态启用 | 已有服务可以演进，不能声称没有后台服务；现有保活也不证明所有设备/Doze 下可达 |
| 文本及复制 | 上游 FileMetadata 在 protocol/types.rs:126–137 无 content_kind；上游 commands 接口无 send_text/接收后复制实现；定制版 commands.rs:374 起 send_text、473 起 clipboard、830–846 prefetch | 上游有“复制票据”，不能误写成已有定制版的文字直传或接收文件复制功能；本方案不顺带移植这些产品功能 |
| 权限边界 | node.rs:74–82 的 PairedOnlyHook 对非 CONTROL_ALPN 直接 accept | 新增流式/元数据 ALPN 必须另加逐传输权限校验；不能仅注册路由就认为受配对 ACL 保护 |
| 发送完成 | protocol/send.rs:287–294 用 95% 阈值，484–556/592–651 根据 provider 请求完成与 500 ms debounce 发事件 | 它不是接收端 durable receipt；新协议禁止用 provider completion 证明对端已保存 |

上表本地源码均可在锁定根目录按相对路径核查；上游永久链接格式为 [b41eddb 源码](https://github.com/tonyantony300/dashbeam/tree/b41eddb16b226667de6f8dd7f1326a0ce6bf5e7d)。

## 3. 正确的优化边界

官方 iroh 1.3.0 的 src/lib.rs:64–78 明确：通常先借助 home relay 建连，建立后探测直连并迁移，也可以同时使用 relay 与 IP 地址。不要再增加“先等穿透失败 3 秒才尝试中继”的应用层串行等待。

官方 1.3.0 src/endpoint/quic.rs:152–163 在 noq 默认值上设置 keepalive、多路径等；src/socket.rs:109/117 分别为 5 秒 heartbeat 与 15 秒 path idle。实际 noq-proto 1.3.0 config/transport.rs:548–586 默认 stream_receive_window 为 1,250,000 B、send_window 为 10,000,000 B、connection receive_window 为 VarInt::MAX、initial RTT 为 333 ms、默认 Cubic。以上是当前源码值，不是本机实际已协商参数、测得带宽或给所有连接加大内存的理由。

QUIC ACK 只确认对端 QUIC 收包处理；不证明应用解密、校验、写盘、fsync 或最终导出。普通 relay 只转发密文 QUIC 包，既没有应用密钥，也不是原始连接的接收端。因此“把 relay 收到的 QUIC 包存到硬盘并假装 ACK”不是可靠离线续传方案，且会破坏原连接的包号、密钥、拥塞控制、重传和会话生命周期语义。

新的 spool 通过独立应用协议接收完整密文帧。S→P 与 P→R 是两条独立传输；P 可以终止这两跳的 TLS/QUIC/HTTP 连接，但文件内容和私有元数据仍由 S/R 持有的应用层密钥加密。P 能看到流量大小、时序及用于准入的设备/传输标识，不能据此声称“完全无元数据泄漏”。

## 4. 推荐架构

### 4.1 客户端

1. 一个 TransportManager 持有一个常驻 iroh Endpoint 及网络配置 generation。设置从持久存储完成加载后再 bind；后续配置变更创建明确的新 generation 并使旧健康缓存失效。不要同时启动相同身份的两个 Endpoint：1.3.0 relay 会替换/暂时失活同 EndpointId 的连接，而不只是抽象意义的“多几个 socket”。
2. 节点注册原 CONTROL_ALPN 及新的 dashbeam/session/2。**不同 ALPN 是不同 QUIC 连接，不能在原 CONTROL1 连接中直接塞 STREAM2 数据。**新 ALPN 中统一用 stream-kind 区分会话控制、文件帧、预览附件；同一已认证 SESSION2 会话可复用于多个传输。
3. 每个 transfer_id 都有独立 TransferSession：recipient、consent、source handles、manifest、keys、epoch、progress、cancel token。禁止沿用全局单一 Option<FileMetadata> 来回答并发预览。前端事件都带 transfer_id、generation、单调 event_seq，旧任务事件不能改写新任务 UI。
4. SourceReader 抽象提供 Read、可选 ReadAt、可选 StableVersion、Cancel。桌面文件句柄、Android URI、目录增量枚举都接入这个边界；不要求先转为本地完整文件。
5. SinkWriter 提供 prepare、write_verified_frame、durable_checkpoint、commit、abort。默认写目标同卷隐藏 .part，完成验证后原子改名；不再默认“store 完整文件→复制到目标”。
6. 可选 SpoolClient 使用同一帧格式，与原 iroh relay 分离。建议独立 HTTPS/2 流式服务，通过常见 443 路径到服务器；文件 E2E AEAD 与底层 HTTP 版本无关，不要求重写 iroh relay。服务加入现有服务器前必须单独审批资源、进程和入口变更。

SESSION2 初期仅在用户活跃的发送页、正在传输及近期实际联系的目标上预热；不对所有历史配对设备永久多开一套数据连接。已升级双方稳定运行后，用 SESSION2 的认证控制与 presence 代替同一对设备冗余的 CONTROL1 长连接。旧设备继续走原控制路径。

具体发送体验：设备列表进入发送页就从已认证 presence 缓存呈现，并标出新鲜度，不等文件导入或 share ticket。用户选源后即可并行打开源、预读至多首帧和准备目标连接。目标已经选定且接收端此前明确设置了适用的自动接收授权时，OFFER 到达后由接收端重新核对设备、类型、大小、目的地和授权期限，自动返回 ACCEPT，随后立即发送首帧；不再让用户每次点一次确认。普通配对不自动等于同意，未知或不在授权范围内仍要求接收端确认。未选目标时“开始分享”立即显示现成目标列表；选择期间单独计时，不能发送给猜测出的对象。整个 SendIntent→首帧回执仍纳入报告，不能靠排除人工等待来宣称体验已优化。

### 4.2 正常时序

~~~mermaid
sequenceDiagram
    participant UI as 发送端界面
    participant S as 发送端会话/SourceReader
    participant R as 接收端会话/SinkWriter
    participant P as 可选密文spool硬盘
    UI->>S: 创建transfer，选择源和目标
    par 目标/连接准备
      S->>R: 复用SESSION2或连接并首写OPEN
    and 轻量源准备
      S->>S: 打开源，取首项基本信息，读首帧
    end
    S->>R: OFFER(认证的最小元数据、范围、能力)
    R->>S: ACCEPT(同意、目标准备结果、限额)
    S->>R: FILE_OPEN + FRAME(首8KiB，独立AEAD)
    R->>R: 验证帧tag，写.part
    R-->>S: VERIFIED_PROGRESS
    R->>R: fsync数据+持久checkpoint
    R-->>S: RECEIVER_DURABLE
    opt 连接/速率解耦确有需要
      S->>P: CREATE/APPEND密文帧
      P->>R: 首帧可用即转发，不等4MiB/20%
      P->>P: append+sync+commit journal
      P-->>S: UPLOAD_DURABLE
      R-->>P: 经身份签名的CONSUME_DURABLE
      P->>P: 持久化消费水位，回收已消费段
    end
    S->>R: FINAL_MANIFEST(文件根/长度/块数/集合承诺+签名)
    R->>R: 完整校验、sync、提交目标
    R-->>S: FINAL_RECEIPT(verified/durable/exported)
    S->>S: 关闭源并清理恢复材料
~~~

缩略图和 MIME 深度探测在 OFFER 后低优先级发送，失败不阻塞文件；初始 OFFER 有文件名/根项、文件或目录类型、已知大小/未知标记、数量是否完整、总量上限、内容版本语义及有效期。目录增量 FILE_OPEN 必须属于用户所选根；扫描未结束时显示“正在整理文件”，最终集合必须核对，不能默默跳过失败条目。原选择范围内任一条目枚举/读取失败，原 offer 不得产生完整 verified=true 的 FINAL_RECEIPT 或完整 Completed。可保留已校验的文件并显示 Partial；若用户愿意只收子集，需要明确确认并用新的 offer/transfer_id 绑定修改后的集合，不能把失败清单作为无提示缩减选定范围的借口。

### 4.3 路径选择

- 有已认证 SESSION2：立即走它，沿 iroh 已有 relay/IP 路径发送，直连迁移交给 iroh；没有额外“等待直连”。
- 冷连接：打开 SourceReader 与 SESSION2 connect 并行。若 300 ms 后仍无可用数据路径，且已配置并授权 spool、已知其可用、双方支持 E2E 控制信封，则启动 spool 候选。300 ms 是首轮试验参数，不是连接失败判据。
- 只竞速一个首帧（试点 8 KiB，上限 64 KiB 加协议开销），接收者以 transfer/epoch/frame_id 去重；得到首帧认证结果后固定主路径并取消输掉一方的 bulk。禁止整文件双路重复上传。
- spool 工作时，iroh 仍可在后台建连；验证到可用直连后从接收端持久水位切换，同一 epoch、同一帧身份。至少 5 秒滞回并依据实测 RTT/有效吞吐决定，避免抖动切换。切换不重置进度，不跳过缺口。
- 中继严格模式、DIRECT/网络偏好、用户关闭缓存的设置保持权威；未经选择不回落公共服务。P 不可用时可继续 P2P，不能自动把数据交给另一个未授权服务。

## 5. 状态机与消息契约

### 5.1 状态机

~~~text
Created
  -> PreparingSource + ResolvingPeer（并行）
  -> Offered -> AwaitingConsent
  -> Accepted -> Streaming
  -> AwaitingFinalManifest -> Verifying -> CommittingDestination
  -> Completed

Streaming -> PausedSource / PausedReceiver / PausedQuota / Reconnecting
           -> ResumeNegotiation -> Streaming

任意未终结状态 -> Cancelled / Expired / Revoked / Failed / Partial
Partial 为原offer的终结状态，子集继续须有重新确认的新offer
~~~

prepare、connect、hash/读取、accept 等每个阶段都有可取消句柄。3 秒未达到 SLO 应报告正在等待的阶段，不能把“仍在整理目录”显示为“网络传输 0 B/s”，也不能制造成功。一次 connect 操作拥有一个总 deadline，候选路径共享这个预算；不存在层层 3 次重试相乘。建议初始建路软提示 3 秒、一次建路预算 10 秒，用户可重试；数据阶段按源/目的地/网络实际进展诊断，低速或 provider 长读不能仅因“3 秒”被中止。

### 5.2 会话、授权与控制消息

首轮协议使用固定长度上限的版本化二进制头与规范化 CBOR 消息，签名对象规定唯一编码；禁止直接签未规范化 JSON。控制消息默认最多 64 KiB，预览附件另流且有预算；大目录 manifest 分页并做根承诺，不用无限大的 OFFER。

| 消息 | 必要字段及含义 |
| --- | --- |
| OPEN / HELLO | protocol_version、session_nonce、endpoint_id、capabilities、最大帧/并发/元数据上限、key_id、会话绑定证明；打开 bi stream 后立即首写，不能双方等读 |
| OFFER | share_id(逻辑选择集合)、transfer_id(128-bit随机)、sender/recipient、epoch、expiry、offer_digest、选定根摘要、已知F/unknown、manifest_complete、源版本语义、允许的路径、接收大小上限；通过已认证会话或认证加密信封 |
| ACCEPT / REJECT | transfer_id、offer_digest、recipient、epoch、同意的范围/大小/路径/有效期、sink状态；接收端身份签名；同意绑定具体 offer，不能跨任务重放 |
| MANIFEST_PAGE / FILE_OPEN | file_id、规范化相对路径、类型、已知size、source_version、页序/前页摘要；终结前允许新增经过范围/配额检查的项目 |
| PREVIEW | transfer_id、file_id、版本、mime/thumbnail；低优先级，内容长度受限，不触发单独预览握手 |
| PROGRESS | sent / received / authenticated / receiver_durable / exported 分开的单调字节水位；页面主进度选择 receiver_authenticated，完成以 FINAL_RECEIPT 为准 |
| RESUME_QUERY / RESUME_STATE | transfer/epoch、offer digest、双方日志 generation、R持久前缀/有限缺口、P当前仍持有范围/lease；不得凭发送者内存进度恢复 |
| CANCEL / REVOKE | transfer/epoch、取消原因、签名、控制序号；幂等，撤销 token 并关闭所有路径 |
| FINAL_MANIFEST | 选中集合最终成员及其受认证根、实际长度、帧数、空文件/空目录、排除/失败清单、总字节、offer/分页链摘要、sender签名 |
| FINAL_RECEIPT | transfer/epoch、final_manifest_digest、recipient、verified=true、durable=true、destination_state(exported/private_staging)、最终字节数、签名；只有原offer完整履行可发，Partial另发不带集合verified=true的终结回执 |

身份可信来源是已配对的 EndpointId/经用户核对的指纹或二维码，不是显示名、mDNS、可访问服务器、在线列表或 relay token。新数据协议逐请求验证 recipient、transfer_id、offer digest、epoch、期限和 consent；同一已配对设备也不能枚举所有传输或读取未向它授权的文件。旧 bearer BlobTicket 能力隔离在 legacy 临时端点，不能把新传输的明文或密钥发布到 legacy store。

认证元数据内联在 OFFER/ACCEPT 所在控制流，收到 OFFER 即可呈现；不要引入“为了省一次 metadata handshake，先建立另一条 metadata handshake”。SESSION2 首写 OPEN 同时承担原 metadata 一字节 marker 的 stream 可见性作用。

### 5.3 数据帧与完整性

首版单文件默认一个 bulk stream，另一个高优先级控制流；少量小文件可打包多个完整文件帧，但保留每文件边界和最终长度。最多 2 个 bulk stream/传输，必须由实际高 RTT 测试证明并发收益后才扩大。64 KiB 是**单独 AEAD 的明文最大值**；试点首帧默认 8 KiB（文件不足则取实际长度），其后 bulk 帧默认 64 KiB，末帧可以更短。显式 offset/len、帧序、nonce 与内容根必须覆盖变长帧边界。空文件使用 FILE_OPEN/FILE_END，不发送虚构字节；普通大文件不发1字节占位包来刷首字节指标。

帧头包括 protocol/transfer/epoch/file_id/frame_seq/plain_offset/plain_len/cipher_len/flags；整个头及 offer_digest、recipient 绑定为 AAD。每帧独立 ChaCha20-Poly1305，tag 为 16 B。接收者完整读到帧及 tag 并验证后才把明文交给 SinkWriter，不能流出未认证明文。4 MiB 只用于硬盘段聚合，绝不将 4 MiB 仅配一个 tag：在 1 Mbps 上 4 MiB 单段仅序列化就需约 33.55 秒；64 KiB 约 0.524 秒，若两跳逐帧转发且两跳都为 1 Mbps，首帧序列化约 1.049 秒，另加建路与 RTT。

8 KiB 首帧把上述纯序列化开销降至单跳约 65.5 ms、两跳约 131.1 ms，16 KiB 候选约为其两倍；这是计算值，不是网络实测。实验同时记录首个真实认证帧和累计至少64 KiB认证内容，防止仅首帧好看、后续数据仍堵住。8 KiB 后接64 KiB整帧时，后一指标首次触发在累计72 KiB，报告按72 KiB实际流量计算。8/16/64 KiB首帧的选择保留为独立可回滚参数，按真实链路、IO和额外小帧开销A/B确认。

FirstAuthenticatedPayload 只证明该帧通过双方授权密钥上下文的 AEAD 验证，不代表所有文件完整，也不是发送者的独占签名证明。首版每个接收者使用独立 root key 和密文，不为服务器去重让多个接收者共用 content key；否则任一接收者都能生成其他接收者会接受的 tag。最终发送者签名承担完整来源与集合承诺；FinalVerified 前不自动打开、执行或交给复杂文件解析器处理未完成文件。

每文件按序计算具域隔离的 Merkle/BLAKE3 内容承诺，叶子绑定 file_id、seq、offset、length 和明文字节，最终根绑定帧数和总长度；定义空文件根。集合根绑定规范化成员与每文件根、选定根范围和空目录。接收者只有验证最终签名、每文件完整覆盖无缺口/重叠、文件数/总量/集合一致后才能 FinalVerified。实现前冻结根算法/规范编码/跨语言测试向量，不能凭“最后一个帧通过 AEAD”当整文件完成。

### 5.4 密钥、nonce 与重放

每个传输使用系统 CSPRNG 产生独立 256-bit root key；按 transfer_id、epoch、file_id、方向、用途通过 HKDF 分离 data、metadata、control key。data nonce 为每 file key 下固定 32-bit 域加 64-bit frame_seq，序号绝不回退或复用给不同明文。文件、方向、epoch 使用不同 key，控制消息不与数据共用 nonce 空间。

正常已有 S↔R 认证会话内可安全传递 root key。为使 spool 在 S↔R 直连/普通中继不可用时仍能承载控制，增加由已配对 Ed25519 EndpointId 签名绑定的独立 HPKE X25519 公钥；禁止把 Ed25519 公钥擅自当 X25519 公钥。使用经审查的 RFC 9180 库，DHKEM(X25519,HKDF-SHA256)/HKDF-SHA256/ChaCha20Poly1305，加发送者身份签名与 recipient/offer/expiry 绑定。P 只转发封装后的信封；CREATE 成功后需要恢复的信封才可存盘且计入 Q。旧配对记录没有认证 HPKE key 时不能走该控制降级，需先成功建立新会话或升级配对。

首版不提供未接受任务的服务器离线信箱：OFFER/ACCEPT 可经受限的在线控制通道转发，只有固定小网络缓冲，离线则由发送端等待。它不缓存文件字节，也不绕开 F=0、未知F或小文件的磁盘规则；因此 CREATE 要求已有 ACCEPT 不会造成“必须先CREATE才能发送ACCEPT”的循环。

HPKE 本身不自动提供应用重放防护、消息顺序或长期前向保密。协议用 transfer_id、epoch、单调控制序号、持久化终结/tombstone 防重放；密钥轮换和旧 key 解封期限明确，旧 key 到期会使离线信封失效。会话转移/重连不重新生成同身份同 epoch 的不同 root key；重传首选原字节密文。无法确认崩溃前 nonce/明文映射时终止该 epoch，建立新传输，不能继续猜测。

首版不对 OFFER/ACCEPT/授权数据/DELETE/CREATE 开启 0-RTT，也不在服务端客户端身份尚未完成验证的 0.5-RTT 阶段发送私有数据。优先复用已认证会话来省时。

## 6. 硬盘滑动窗口 spool

### 6.1 配额与小文件边界

F 以 byte 为单位；这里 1 GB 明确定义为 1,000,000,000 B。若产品采用 1 GiB，必须更改文档与协议参数，不能混用。C 是硬盘占用上限，不是 RAM allocation，也不是“最多只发送整个文件的 20%”。

服务分配 Q <= C(F)，实际占用 D 加未落盘预留 credit R 始终满足 D + R <= Q。D 包含尚未被接收者持久确认的密文、tag、段头、索引、WAL、临时文件、文件系统已分配块，以及等待 unlink 完成/仍被 fd 引用的块。不能只统计明文长度或磁盘文件的逻辑长度。

- F=0：不使用 spool 数据窗口，只通过受限控制消息记录空文件。
- 首版 F < 1 MiB：直接/P2P，不用 spool；避免文件系统块与元数据大于严格 20% 上限。该阈值是保守工程门槛，之后可按实测单独调整。
- F 未知（云 provider、pipe、未枚举完的目录）：先直接流式传输，明确显示未知大小；不先按 1 GB 缓存。只有取得可信的确定总量且仍符合配额后，后续帧才可启用 spool。目录不用为了配额反过来阻塞首帧。
- Q 不足以容纳当前帧与必须持久化的元数据时，暂停上传或采用直接路径；不突破比例，也不永久等待“凑够 4 MiB”。
- 多文件一次分享用确定的集合 F 计算父级 share_id 配额，禁止每个文件各取 1 GB。多接收者使用独立授权的 child transfer、独立 key/密文/ACK，但所有 child 的 D+R 之和仍 <= 同一 share_id 的 C(F)，不能把一次10GB分享给3人变成3GB硬盘缓存。首版新流式路径只开放单接收者，原广播继续使用明确的 legacy 分支；将来开放多接收者须先实现父级配额与公平调度，设备/全局配额仍另外生效。

示例：100 MB 文件最大 20 MB；1 GB 文件最大 200 MB；10 GB 文件最大 1 GB。若上传 100 Mbps、消费 10 Mbps，1 GB 窗口约在 1,000,000,000/(12,500,000−1,250,000) ≈ 88.9 秒填满，之后背压把长期吞吐压到慢端速度。缓存不能改变链路瓶颈；接收者完全离线时，100 Mbps 上传约 80 秒就填满 1 GB。

全局容量另设硬上限 G、设备限额 U、并发槽和文件系统 reserve。主负责人提供的当前背景为空闲盘约 GZ 20 G / SH 28 G、每客户端输入限速均 12,500,000 B/s；这是每客户端入口限制，不是两台服务器的总出口带宽。本任务未重新测这些服务器，不据此自动部署。试点建议只在一个已授权服务节点启用 G <= min(4 GB, 启用时可用空间/4)，同时至少保留 10 GiB 或文件系统容量的 20%（取较大）给系统，达不到则禁止新任务；最多 2 个活跃上传/设备，reservation 计入全局。具体值须依据文件系统容量、其他服务和真实 IOPS 审核。当前 ingress 100 Mbps 量级限速下，提高客户端带宽窗口不能突破服务器限流。

### 6.2 段与内存

服务器 append-only 段最大 4 MiB，每段可含约 64 个 64 KiB AEAD 帧；Q 小时缩小段或使用单帧段，禁止预分配超出 Q 的 4 MiB 文件。完整首帧可用即转发，不等段满、fsync 或 C 满。落盘任务按“最多 4 MiB 或 50 ms 先到者”提交一批；首帧可以单独提交。这个 50 ms 是设计参数，不是承诺底层盘一定在 50 ms 完成 sync。

每传输应用 RAM 用独立且小的预算：例如 8 帧队列（约 512 KiB）加固定控制缓冲；服务器所有会话全局内存上限另设。QUIC/HTTP 库、TLS、内核页缓存仍占 RAM，必须测实际 RSS/pressure；不能把硬盘窗口伪装成 1 GB ByteArray，也不能因用了 write() 就说没有内存成本。

准入预算还必须计入 max_concurrent_bidi/uni_streams、每 stream 接收窗、connection 接收窗、send_window、已认证/待握手连接数及 HTTP/2 流控/队列；不得沿用默认 100+100 streams、VarInt::MAX 总接收 credit 后仅凭应用 channel 有界宣称内存有界。P2/P4 放行前为这些值设置明确上限，核算“所有允许连接同时吃满窗口”的峰值并压测；窗口的最终数值按目标 BDP 与设备预算确定，不能拿硬盘 C 作为 QUIC 内存窗口大小。

### 6.3 上传、消费与回收协议

| 操作/ACK | 语义与持久条件 |
| --- | --- |
| CREATE | 验证上传设备身份、接收者同意、share_id/transfer/epoch/offer、F、TTL、父级/设备/全局配额；返回 spool_id、Q、lease、credit；设备凭据不放票据或日志 |
| APPEND | 传精确密文帧及 digest；幂等键 = transfer/epoch/file/frame_seq；相同键不同 digest 立即冲突失败，不能覆盖 |
| UPLOAD_DURABLE | 仅在段数据与校验信息完成 sync、journal/index 原子提交且必要目录元数据持久后返回；只证明 P 持有这些帧，不证明 R 收到 |
| READ / FORWARD | R 携带 recipient 绑定能力读取；返回当前仍实际持有范围，首帧即转发，R 可因帧 AEAD 失败拒收 |
| RECEIVER_DURABLE | R 完成帧 AEAD/顺序范围检查，写 .part 后 sync，并持久化一致 checkpoint；签名绑定 transfer/epoch/offer、消费水位、已消费内容根、receiver journal generation |
| CONSUME_DURABLE | P 验证上述 R 签名与范围，先持久化消费水位，才回收全段；普通 TCP/QUIC ACK 或 S 自称“已收到”均不能触发回收 |
| CREDIT | 只有实际释放磁盘占用后发放新 credit，防止慢 unlink/打开 fd/崩溃后重复计算 |
| FINISH | 保存 final manifest 密文及状态；仅在最终 R receipt 或到期/取消后进入终结清理 |
| DELETE / EXPIRE | 幂等关闭读写，持久化 tombstone，清理数据、索引、密钥封装与租约；返回已释放量，不伪称安全擦除 SSD |

R 的消费签名不必每帧一次：默认至多每 1 MiB 或 50 ms 一批，并含连续前缀及有限缺口。P 自己不能伪造 R receipt；同一份经过认证的水位可同时经直接控制和 spool 回传。强制按有界窗口管理缺口，不能让攻击者用无限稀疏帧号撑爆索引。

服务滚动压缩 checkpoint：只保留当前窗口内索引和一个已消费前缀/根，已回收帧不永久留一条数据库行。否则数 TB 传输的历史索引也可能突破 1 GB。WAL/compaction 的临时双份空间事先计入 reservation。

窗口满时，停止授予 APPEND credit → 停止读取上传流 → 客户端停止加密/读源；正常背压落到源头。不能用 RAM 暗存超限数据、覆盖最早的未消费帧、丢数据后仍 ACK，或让重试创建新任务绕过配额。

### 6.4 TTL、崩溃与离线承诺

建议未接受 OFFER TTL 5 分钟、暂停且无实际进展租约 15 分钟、最长任务租约 24 小时；仅实际数据/已认证消费进展可续租，单纯 heartbeat 不能无限占盘。超长合法传输接近到期时需双方在线续签新租约，不能偷偷将旧租约永久延长。

P 重启按 journal 扫描帧长度/digest，丢弃未提交尾部，重建 reservation，返回持久水位；尚未发 UPLOAD_DURABLE 的帧可以重传，已经 ACK 的帧必须能恢复。R checkpoint 的持久顺序必须是“数据 sync → checkpoint 原子提交与 sync”；反过来会在崩溃后谎报已保存。过期状态须持久化，恢复不得把终结任务复活。

**首版不把 UPLOAD_DURABLE 显示为“发送完成/可以关闭发送端”。**C(F)<F 的滑动缓存通常只持有一段；源端必须保留源/恢复材料，直到接收端 FINAL_RECEIPT。允许接收者短时断线后从窗口补取，窗口耗尽就暂停。未来若做“发送者提前离线”，必须证明 R 的持久前缀与 P 当前持久且未过期尾部的并集覆盖全部内容及最终 manifest，并处理 TTL/灾难恢复；本版不实现这项更强承诺。

## 7. 源变化、恢复、取消及目标落盘

### 7.1 源版本

TryReference 不提供不可变快照。官方 iroh-blobs 0.103.1 api/proto.rs:630–644 明确它假设导入后的文件不再变化。新设计不能用读取前后 size/mtime 相同作为点时快照的证明。

首版明确两种 SourceVersion 语义：

- ImmutableVersion：provider/文件系统提供可验证的不变版本、持有的稳定对象或有明确一致性保证的快照。若采用 APFS clone、Linux reflink、Windows clone 等快路径，要核对具体文件系统与并发写语义；不支持时不能虚称快照。
- CapturedStream：发送的是一次读取实际取得的字节序列，最终承诺绑定该序列。持有原始句柄，检测到截断、已知版本变化、文件替换或读取错误即失败；stat/监视器仅为变化检测，不保证抓到同大小快速回写。需要“某一瞬间原文件快照”的用户必须使用 ImmutableVersion 或先做完整本地快照，这类准备时延不进入快速首字节 SLO。

每帧在首次发出前固定其身份与明文摘要；恢复材料必须证明再次发送完全相同的字节。直接及 spool 重复路径复用同一密文。可重读的 immutable source 可按 offset 重建，但须核对已记录摘要；不可重读的 URI/pipe 需要本地磁盘保留未被 R 持久确认的帧，或在不支持恢复时如实标记。若本地恢复预算也用完，背压停止 SourceReader；不能悄悄保存整个源文件却称“没有前置复制”。

源进程崩溃后若无法证明原 epoch 的 frame/nonce 映射、未确认帧及累计内容承诺，则该传输标为需要重新发送。不得拿新源数据填旧帧号。最终 hash/内容根计算应在正常读取时增量完成；崩溃恢复若缺可靠摘要 checkpoint，可以重新读源校验，恢复耗时不属于正常首帧 SLO。

### 7.2 接收目标

桌面默认在用户选定目录同卷的私有 .part 内按 offset 写；校验全部内容后 sync 并原子改名，同名冲突在 prepare 阶段安全预留名称。使用目录句柄相对打开、禁止 symlink 跟随/路径穿越/Windows 保留名与大小写碰撞，不能只用字符串过滤后再普通路径打开。目标不支持原子提交时保留明确“已接收，正在保存”状态。

Android：

1. 发送选取返回受控 SourceHandle/URI + 临时/可持久权限信息，Kotlin ContentResolver 流式读到 Rust 的有界 channel 或可取消 FD；不把 64 KiB 大块经 JSON/base64 逐块过桥。
2. 先探测可 seek 能力，不能假定 content:// 对应普通文件。ContentResolver 的 r 模式可能返回 pipe/socket；不为“得到 seek”擅自索要 rw 写权限。云 provider 首读、未知长度、权限过期、重新打开不保证同一内容，均须有独立状态与恢复限制。
3. 可可靠恢复的目标优先直接写受控 app-private .part；MediaStore 可采用 IS_PENDING 方式逐帧写入，最终校验后发布。SAF provider 是否支持定位写、fsync 与原子/可撤销提交需要逐类验证；不具备时保留 app-private staging 并在完成后导出，接受这段尾部复制而不虚报用户文件已就绪。
4. 增加任务前台服务所有权，传输期间的 SourceReader、网络和 sink 不随 WebView/Activity 销毁直接中止；合法启动服务、通知、系统后台限制、用户划掉、Doze、电池优化都进入真机验收。现有 PresenceService 的 connectedDevice 类型和永久低时延 Wi-Fi lock 不能直接当所有云 spool 数据任务的正确服务声明。
5. 高性能 Wi-Fi/CPU wake lock 只在活跃传输且必要时持有；空闲设备列表使用较低成本 presence，应用退出/用户停止应释放。后台“在线”标识必须和可接收能力一起报告。

取消有两个不同产品动作：Pause 保留受 TTL 限制的恢复状态；Cancel/Delete 在本地立即撤销权限、停止源读取并清理约定范围。远程 P/R 的撤销需要收到其确认或租约到期，P 不可达时显示“远程清理待确认”，不能宣称已立即删除远端副本。终结状态一旦持久提交，迟到的 APPEND、ACK、RENEW、COMMIT 均不能复活旧任务。不能沿用“取消也保留”却不给用户说明。撤销配对立即停止新 Offer/Accept、当前接收许可按策略撤销并关闭数据流；已经被合法接收并保存的明文无法远程撤回，服务删除密文也不能撤销对方已拿到的密钥。

## 8. Presence、设备列表与电量

保留当前上游“控制连接派生 presence”的优点，但拆成 connection_seen、authenticated_session、receiver_ready、background_limited、stale 五类证据。当前 online=true 只说明近期控制会话存在；UI 不保证文件可发送，不把 stale 简单当永久离线。选中目标可进行一次主动、共享 deadline 的 OPEN/OFFER；最终“可接收”以该接收者的 ACCEPT 为准。

可选服务器在线目录只提供经过设备认证、限定配对关系可见的 lease hint（例如 15 秒更新、45 秒到期，后台节能可变），不维护公开所有设备列表、不凭显示名认人。epoch、签名和接收端新鲜响应防止服务器/旧缓存替冒“已接受”。服务器宕机时局域网发现与已知 iroh 地址仍可工作。

统一 Endpoint 降低重复 relay 连接与探测成本；保活不能无上限扩张。先复用上游 control presence，再测一个新会话长期保持与按目标短暂预热的电量差。由于 iroh 1.3.0 有 path heartbeat，不在无测量情况下再加每秒应用 ping；低电量/后台模式允许失去热连接，清楚标记从“热SLO”退为冷连接。

## 9. 首字节指标与预算

以同一 transfer_id 关联事件，但各设备单调时钟的 epoch 不同，不能直接用 R 的 FirstAuthenticatedPayload 时间减 S 的 SendIntent 时间。正常报告两个可复核指标：(1) R 本机 ACCEPT 发出→首帧完成 AEAD 验证；(2) S 本机 SendIntent→收到经认证的首帧确认回执，后者包含回程 RTT，是跨端首帧时延的保守上界。该上界 <3 s 可证明实际首帧更早发生；上界 >3 s 不能反推出单程首帧也超过3 s。需要跨端单向分解时，用受控同步和时钟偏差置信区间，不能只靠关联id或普通壁钟相减。

| 指标 | 定义 | 目标/限制 |
| --- | --- | --- |
| UIReady | 点击后传输卡片与可取消状态可见 | 本地 p95 <= 100 ms；不等价于已有网络字节 |
| OfferVisible | 目标收到并验证最小 OFFER，可显示同意界面 | 热会话 p95 <= 500 ms；目录完整列表可继续到达 |
| FirstCiphertext | 数据首字节发到当前传输路径 | 只表明开始发送，不能称接收者拿到文件 |
| FirstAuthenticatedPayload | R 收全首帧、验 tag，第一次产生可信明文 | 适用条件下，接受后 p95 <= 3 s |
| First64KiBAuthenticated | R累计至少64 KiB真实内容完成AEAD验证；文件不足时用其完整实际内容 | 与首帧一起报告；8+64 KiB帧布局触发于72 KiB，不把网络字节等同认证字节 |
| FirstDurablePayload | 首帧在R目标/staging及checkpoint持久 | 在上项基础上加真实磁盘 sync 时延，单独测 |
| FinalVerified | 所有文件/集合通过最终承诺校验 | 随文件大小与有效吞吐增长，无固定3秒 |
| UserReady | 已提交最终目标且用户可打开 | 不早于 FinalVerified；Android导出单列 |
| SenderComplete | 收到R签名 FINAL_RECEIPT | 不以 QUIC ACK、P上传完成、95%阈值替代 |

热路径首帧预算（设计预算，不是测量）：

| 阶段 | 条件/预算 |
| --- | --- |
| 现有已认证会话取用、控制排队 | <= 100 ms |
| 接收者接受后目标准备、源首读、密钥/首帧编码 | 并行后关键路径 <= 300 ms，且源非冷云文件 |
| 控制与数据 RTT/调度 | <= 400 ms，端到端RTT <= 150 ms、无持续重传拥塞 |
| 8 KiB 首帧传输 | 单跳1 Mbps约65.5 ms；两跳逐帧1 Mbps约131.1 ms |
| 解密与本地排队 | <= 100 ms |
| 额外抖动余量 | 700 ms |
| 首帧合计 | 单跳约1.666 s；两跳约1.731 s，目标<=3 s |
| 累计至少64 KiB的保守预算 | 8+64 KiB共72 KiB，按两跳逐帧序列化相加约1.180 s；相同其他预算下约2.780 s。真实流水转发可重叠，但不预先把重叠收益算作保证 |

以上预算需要：双方在线可运行、新协议及认证密钥已协商、用户同意已完成、有效可用速率至少 1 Mbps、约定 RTT/丢包范围、源能及时给出首帧、sink没有满盘/权限阻塞、P有配额且无慢盘长停顿。不能覆盖休眠设备唤醒、手动确认、冷启动、全目录清点、云 provider 拉取、不可变快照准备、磁盘异常或 legacyBlobs。双跳第一帧预算采用完整帧到达才转发的保守计算；不靠未认证明文流出来“达标”。

测试开始前冻结 eligible 矩阵（文件来源/大小、目标类型、前后台、热/冷、连接模式、RTT/限速/丢包、授权模式），不能事后因为失败就说“源不够快”而剔除。所有 eligible 失败与 timeout 都计入成功率和 SLO 分母；同时报告 SendIntent→首认证回执全程、R ACCEPT 后阶段、用户选设备/确认区间、冷启动、legacy回退及各类样本数。样本很少时只报告每次测量/范围，不宣称已有可靠 p95。

正常总耗时近似：

T_user_ready = T_unoverlapped_prepare + T_consent + T_path + F / min(source_read, encrypt, uplink, path, downlink, decrypt, sink_write) + T_unoverlapped_verify_commit

流水线只能减少未重叠准备/导出和速率波动，不能突破 min(...)。spool 除读写加密外，还增加服务器网络和两次磁盘持久化，不能默认提高总完成吞吐。文档和界面同时显示“全程耗时”“数据阶段耗时”，不再只展示 payload execute_get 的短时间掩盖前面等待。

主负责人提供的历史旧版样本：1,499,219 B、wall 45.416 s、payload_get 3.253 s、export 17 ms，差约 42.146 s。它只证明存在大量非 payload 时间，不能唯一归因给探测、hash、Android复制或某个连接阶段；不作为新上游耗时实测。

## 10. 兼容与安全验收边界

新 ALPN 与新传输格式带版本号，旧 BlobTicket 解析器不解释新 ticket。旧设备能力确认失败若是明确“unsupported ALPN”才缓存为 legacy；网络超时不能当作不支持版本，也不能静默降级以绕开安全策略。向旧 CONTROL1 不发送它无法解析的新 enum variant；可选能力字段需兼容性测试，或先做独立新 ALPN 探测。

与旧设备通信走明确的 LegacyBlobs 分支：保留导入/hash、现有文件校验、旧票据及单独权限模型；界面显示正在准备，**不承诺新协议首帧目标**。未迁移的浏览器 wasm 内存 store 不得突然继承 native 磁盘承诺；Web 流式/OPFS/后台生命周期另设支持等级，首期可只留 legacy。

新的安全门槛包括：跨recipient重放、跨epoch帧替换、同序号异密文、目录路径攻击、巨大metadata/稀疏seq、未接受就上传/读取、恶意中继伪造消费ACK、spool丢/改/重排帧、0-RTT重放、撤销后旧token、源变更、部分末帧与缺失finalmanifest。身份、consent、每帧完整性、完整文件承诺、磁盘durability分别验收。

日志只记匿名/截短关联id、版本、阶段、计数、时间和错误类别；不记票据、访问token、root key、HPKE私钥、完整文件名或文件内容。debug bundle仍需受控脱敏。P只在固定spool根内操作，不接受客户端路径，data与index权限分离，TLS身份验证保留，不能为降低握手延迟关闭证书校验。

## 11. 分期最小改动、依赖与回滚门槛

| 阶段 | 最小 patch 范围 | 依赖 | 通过门槛与回滚 |
| --- | --- | --- | --- |
| P0 基线观测 | protocol/send.rs、receive.rs、relay.rs；native/send.rs、import.rs、receive.rs；commands.rs；Android copy/export；前端阶段卡片 | 锁定新上游与旧定制版各自独立基线 | 每阶段起止/取消/错误可关联；不泄密；每种文件的wall=sum/overlap可解释。只加观测，性能回归<=5%；独立开关 |
| P1 关键路径整理 | TransportManager/配置hydration；常驻Endpoint健康；commands元数据拆轻量与异步预览；前端先选目标；取消贯穿准备阶段 | P0证据、并发/生命周期设计 | 无临时probe阻塞发送/接收；没有default→custom短暂错误发布；目录/thumbnail不挡UI；legacy结果正确。先不承诺消除fullhash，feature flag可退回旧流程 |
| P2 SESSION2直连/普通relay流式 | protocol新session/frames/manifest/crypto；native SourceReader/SinkWriter/TransferSession；node router与逐传输ACL；frontend按transfer事件 | 规范冻结、密码库审查、静态/动态fixture、旧协议互通 | 1B/0B/64KiB边界/1MiB/1GiB/多文件/目录完整；源变化与cancel无假完成；规定网络热p95首认证帧<=3s；总吞吐不低于legacy稳态90%；一键只对新任务回退Legacy |
| P3 Android真实流式与目标提交 | native-utils Kotlin SourceHandle/FD/Channel、前台服务所有权；SAF/MediaStore sink；Rust bridge | P2、设备/provider能力矩阵 | 不再等待整URI复制才发首帧；未知长度/pipe/权限撤回/Activity销毁/锁屏/Doze可解释；后台和真正文件管理器打开验收。恢复不了明确失败，保留原SAF复制降级开关 |
| P4 有界密文spool试点 | 新daemon与独立db/段格式、SpoolClient、HPKE key capability、quota/ACK/GC/lease协议 | P2/P3稳定；服务端资源审批；持久化故障注入；E2E审查 | 每次故障/重启/ENOSPC后D+R<=Q<=C；P不能解密或伪造RACK；断开R窗口满后背压；从不误报sender可离线；仅一台试点/两个设备。关闭新准入，存量drain/到期清理后停止daemon，不删其他数据 |
| P5 有证据的调参 | connect预热/hedge阈值、帧队列、有限并发、接收window、fsync批量、电量策略 | P0–P4真实分位与profile | 每个调整有独立A/B；增加吞吐不能显著劣化首帧/尾时延/电量/内存；保持transport defaults作回滚基线 |

不建议把 P1 到 P4 放入一次大 PR：P1 仍是 Blob 数据协议，P2 才消除必须预先完成内容 hash 的入口，P4 才是磁盘暂存能力。每阶段独立可退回；新协议任务已经开始后只能协商恢复或终止，不能在中间把同一流静默改解释为 legacy。

验收矩阵至少包含：LAN直连、强制relay、UDP不可用、不同home relay、100/300ms RTT、0/1/3%丢包、1/10/100Mbps不对称链路、快发慢收、慢盘/满盘、网卡切换、S/P/R在每个ACK前后崩溃、配对撤销、source覆盖/截断、SAF本地与云provider、双方OS组合。先用可丢弃合成文件与隔离测试环境，不读取真实用户文件测试。测试不能只看“网络联通”或一端进度100%，必须比对内容、完整集合、目标可打开、持久水位、spool回收和实际磁盘峰值。

最终放行条件由主负责人结合实测补齐；本报告不是实施授权，也不是上述门槛已经通过的证明。

### 11.1 开发实验的已知边界

主负责人在用户后续允许的开发模拟中完成了独立原型。本稿只读核对 [disk-spool.jsonl](/Users/luoji12103/Codex/dashbeam-macos/reports/transport-audit-20261110/disk-spool.jsonl) 的三条结果：1 MiB 首帧完成 AEAD 验证约 29.57 ms；32 MiB 无限速约 7.37 ms；32 MiB 接收端限 4 MiB/s 时首帧约 6.13 ms、全程约 8.006 s，出现 406 次 credit 背压，帧文件已分配块峰值 6,684,672 B <= 6,710,886 B，最终长度及 SHA-256 一致。小样本支持“边读、边写硬盘、边转发，不必等20%缓存满”的可实现性。

这是 DEV loopback 的 HTTP、真实文件写入、AEAD 和 fdatasync 原型，不是 WAN、原生客户端、Android SAF 或完整 spool 产品。该磁盘统计只覆盖帧文件分配块，没有 WAL/索引/目录耐久性/崩溃恢复；不能据此宣布本稿 D+R<=Q 的完整实现已达标。真实新 ALPN 实验由另一份实验记录描述，需区分明文网络首包、首认证帧和含回执的同端计时，不能把 raw iroh byte 流的结果写成完整产品时延。

## 12. 本次实际读取的外部一手来源

1. [作者上游固定提交](https://github.com/tonyantony300/dashbeam/tree/b41eddb16b226667de6f8dd7f1326a0ce6bf5e7d)；本地读取了上述 manifest、两个 Cargo.lock、发送/接收/import/export/relay/control/pairing/node 路径及相关 Tauri、前端、Android源码。
2. [GitHub releases/latest API](https://api.github.com/repos/tonyantony300/dashbeam/releases/latest)，本次只读取得 v0.7.2、发布时间和target_commitish。
3. [iroh 1.3.0 官方 crate](https://static.crates.io/crates/iroh/iroh-1.3.0.crate)：src/lib.rs:64–110 连接/中继、143–156 首写要求；src/endpoint/quic.rs:152–163/188–249 默认和window语义；src/socket.rs:109–145 heartbeat/path及multipath常量；src/endpoint/connection.rs:522–528、613–618 的0-RTT/0.5-RTT安全边界。
4. [iroh-blobs 0.103.1 官方 crate](https://static.crates.io/crates/iroh-blobs/iroh-blobs-0.103.1.crate)：src/api/proto.rs:623–644，Copy与TryReference安全语义。
5. [noq-proto 1.3.0 官方 crate](https://static.crates.io/crates/noq-proto/noq-proto-1.3.0.crate)：src/config/transport.rs:548–599，实际传输默认参数。
6. [RFC 9000](https://www.rfc-editor.org/rfc/rfc9000.html#section-13.2)，§13.2 包ACK；§4.2 流控与应用消费，不能推导持久落盘。
7. [RFC 8439](https://www.rfc-editor.org/rfc/rfc8439.html#section-2.8)，ChaCha20-Poly1305 AEAD；[RFC 9180](https://www.rfc-editor.org/rfc/rfc9180.html)，所选HPKE suite、§8.1上下文绑定、§9.7应用层顺序/重放/降级与前向保密边界。
8. [Android ContentResolver.openFileDescriptor](https://developer.android.com/reference/android/content/ContentResolver#openFileDescriptor(android.net.Uri,%20java.lang.String,%20android.os.CancellationSignal))，r/w模式可返回pipe或socket，rw才暗示支持seek；这不构成主动申请写权限的理由。

外部源码以流式只读方式读取，没有安装依赖或修改任何实现。旧记忆仅用于提醒区分探测成功与真实传输成功，所有上游事实以上述本次核验为准。
