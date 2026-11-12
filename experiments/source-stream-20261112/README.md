# 直接源文件流式传输实验

这是一个**显式 opt-in、单个普通文件**的新引擎路径，不是已安装客户端的新版本。
默认 GUI、BlobTicket、配对和传输流程没有切换；旧客户端不能接收这种 ticket。
它复用现有 Iroh/QUIC 和中继，不新增服务器文件仓库或磁盘 spool。

## 本轮改变

- 分享前仅打开源文件并检查元信息、可定位性；不预先完整读文件，不预 hash，不导入 blob store。
- 发送时从同一个已打开文件句柄读取；接收时直接写目标文件系统上的私有 `.part`，不再先写 blob store 再完整复制导出。
- 数据在传输中增量计算 BLAKE3；校验长度、摘要和 EOF 后同步文件，原子无覆盖提交。
- 固定 64 KiB 应用读写缓冲，QUIC 每连接窗口默认上限 8 MiB、可配置 64 KiB–16 MiB，最多四个连接任务。这是背压/在途预算，不会按窗口数字立即预分配整块内存，也不是整个进程的 RSS 上限。1 MiB 的早期原型在高带宽高时延链路回归，故不作为最终默认值。
- `StreamReceiver` 可以复用其 endpoint 和同一分享的连接。当前 `StreamShare` 仍创建自己的 endpoint，尚未接入应用已有控制节点。
- 同一源句柄的内容读取串行，避免复制描述符共享游标引起文件混写；最终回执不占用源游标锁。
- 接收请求同时携带 capability 和接收大小预算；认证/双方预算通过后，`Request → Offer + payload → Complete`，不再为文件信息单独等待一次 Accept 往返。`receive()` 调用本身就是同意接收这个预算内单文件，不可用于未经用户同意的自动下载。

准确的性能数据与测量边界见 [报告](REPORT.zh-CN.md)。Android 的描述符辅助接口仅完成隔离编译，见 [Android 记录](ANDROID_SOURCE.md)；现有 picker/JNI/发送路径仍会完整暂存。

## 运行实验 CLI

在 `engine` workspace 中构建 `source_stream` example。Mac 不承担重编译；本轮全部 Rust/Kotlin 编译和网络实验在 Debian DEV 执行。

```sh
cargo build --manifest-path engine/Cargo.toml --locked --release --example source_stream
```

两端都必须使用这个实验协议。直接可达的两台机器可以显式选择 direct：

```sh
# 发送端：保持运行；ticket 文件必须不存在，并按敏感凭据保管。
source_stream --direct send /absolute/source.bin --ticket-file /private/share.ticket
# 接收端：通过可信渠道取得 ticket；输出目录须预先存在且不能被其他用户写入。
source_stream --direct receive --ticket-file /private/share.ticket --output /private/received
```

也可以显式提供自托管网络，两端采用同样的网络选项：

```sh
source_stream --relay-url https://YOUR-RELAY --pkarr https://YOUR-DISCOVERY/pkarr \
  send /absolute/source.bin --ticket-file /private/share.ticket
```

认证只从 `DASHBEAM_RELAY_TOKEN` 环境变量读取，不要把 token 写在命令行、日志或 Git 中。
没有默认公共基础设施回退；direct 不发布/查询 discovery。Ctrl-C 关闭分享，旧 ticket 随即失效；磁盘上的 ticket 文件由用户删除。

默认接收预算 8 GiB，可用 `--max-bytes` 明确覆盖。这不是可用空间预留；磁盘不足会失败并清理本次部分文件。
CLI 可用 `--transport-window-mib 1` 选择低内存窗口，或显式选择 16 MiB；默认 8 MiB。
过小的窗口会限制高带宽/高时延网络，不能把减小缓冲等同于端到端性能优化。

## 约束与取舍

- 这不是内核零拷贝，也不是不可变历史快照。边传边读会检测通常的源文件变化，但不保证抵御同权限恶意修改。
- ticket 是访问授权，持有者可在分享关闭前请求文件。不要公开发布 ticket。
- 目标目录须可信。临时文件在 Unix 上为 0600；最终硬链接提交不覆盖已有文件/符号链接。文件系统须支持同卷硬链接；不支持时明确失败，不偷偷复制整个文件。
- 已同步文件内容，不承诺父目录的断电持久性。提交后回执丢失仍返回已验证文件，不能自动重试生成重复文件。
- 没有续传、自动重试、多文件集合、浏览器兼容、Android provider pipe 支持。旧的兼容链路继续保留这些产品能力。
- 没有给每个文件配置“20%/1 GiB”服务端缓存。现阶段先通过背压直接中继，避免为低时延额外引入全文件暂存、配额、清理和恢复机制。

## DEV 对比实验复现入口

`source_stream_bench.rs` 只能作为 Linux DEV harness 使用，不是客户端。
它依赖 `benchmarks/latency-20261111/instrument.mjs` 的只观察 hook；必须在独立源码副本注入，不能修改已安装产品。

1. 在新建的 `/home/luoji/code/dashbeam-source-stream-20261112.*` 目录归档当前源码；保存兼容基线 commit 和实际源文件哈希。
2. 把 harness 复制到 `source/engine/examples/source_stream_bench.rs`；只执行一次 observation instrumentation。
3. 准备已有 `dashbeam-benchmark:20261111` 镜像和 Iroh relay 二进制、合成文件、专用 TLS（`make-certs.sh`）。不要安装为系统信任，也不要复用生产私钥。
4. 用下面的固定 profile 构建、测试；完成后执行 `record-build.mjs ROOT`。编译日志和实际 instrumented source/binary 哈希均进入 receipt。
5. 先 smoke，再执行 `node tools/run.mjs ROOT LABEL 3`；LABEL 每次不同。汇总使用 `node tools/summarize.mjs ROOT/results/LABEL`。

```sh
export CARGO_TARGET_DIR=/home/luoji/code/dashbeam-macos/src-tauri/target
export CARGO_BUILD_JOBS=2
export CARGO_PROFILE_RELEASE_OPT_LEVEL=2
export CARGO_PROFILE_RELEASE_LTO=false
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=8
cargo test --manifest-path source/engine/Cargo.toml --locked --offline --release \
  -p sendme-native stream::tests -- --test-threads=1
cargo build --manifest-path source/engine/Cargo.toml --locked --offline --release \
  --example source_stream_bench --example source_stream
```

run.mjs 使用独立 internal Docker network、带任务 label 的三个限额容器，只在端点容器内配置 tc/iptables。
relay 强制场景阻断 peer UDP，并核验实际选中路径；无 host 网络规则和生产改动。每个样本之后逐字节比较，清理精确的合成输出目录/ticket，保留成功与失败元数据。
十种条件各三对 A/B，包含 direct/relay、2/80/150 ms、20/50/1000 Mbit、0.5% 丢包、小/大文件、同分享重复接收。
额外的高 BDP 反例是 case 11/12（80 ms / 1 Gbit/s，64 MiB，direct/relay），必须用第五个参数显式选择。
最终十二组命令：`node tools/run.mjs ROOT main-pipeline-8m 3 1,2,3,4,5,6,7,8,9,10,11,12`。
`validate-cli.mjs` 另用合成文件验证 CLI direct 收发、票据 0600、参数 fail closed 和 Ctrl-C 退出；它不访问现有设置/配对。

两档窗口对照可用 `DASHBEAM_BENCH_WINDOW_MIB=2 node tools/run.mjs ROOT profiles-2m 2 7,12`，
再用值 8 和新 LABEL 运行，使用同一个二进制比较；case 7/12 分别为 50 Mbit/s 与 1 Gbit/s 中继。

旧原型也有可复现记录：在独立源码副本先反向应用 `main-to-tunable-bench.patch` 可恢复最终十二组的固定窗口 harness；
再将 `pipeline-1m-to-final.patch` 反向应用可恢复无 Accept/1 MiB 版本；
再反向应用 `accept-to-pipeline.patch` 可恢复带 Accept 的早期原型。后者使用 `revisions/run-accept.mjs` 和
`revisions/summarize-accept.mjs`，前者使用当前 runner。构建 receipt 指纹对应实际测量源码及二进制，不拿最终源码冒充旧实验。
这是有界工程比较，不是功耗测量或互联网“三秒内必达”的承诺。
