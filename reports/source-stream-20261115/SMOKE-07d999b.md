# 冻结源码最终 13-case smoke

日期：2026-11-15（Codex）；完成时间`2026-11-15T21:08:54.117Z`。

结果：**13/13通过，进程退出码0**。其中12项为完整接收与合成内容校验，
1项为预期取消及空输出清理；不是13次完整传输。
本次只运行一轮，取得结果后仅整理既有receipts和清理状态，没有重跑、增加测试或重新编译。

## 来源与执行范围

- 冻结产品：`07d999b479e25657e6374e413210a0c086c0b853`。
- 已构建DEV探针：`artifacts/final-0.7.2-70201/device_probe`。
- 探针SHA256：`8e76f01515fbf87b9fc629d1aef19b1cbd7b4c51803c107cbc0e8a1cea7e3dd2`。
- 使用冻结提交原有`engine/examples/device_probe_smoke.mjs`，未修改driver或产品。
- driver SHA256：`53bdb4ef0ee4e4f58d0d85b8766843811cb967f82ab413212d42ddf3502bb1e3`，
  与`git show 07d999b:<path>`完全一致；执行前889个tracked文件的manifest校验通过。
- 两个新临时peer均位于DEV隔离任务，TTL300秒；未触碰旧脏checkout、原用户配对或身份。
- 网络配置仅从Mac有效`settings.json`中的`app_settings.state`读取，按应用现有IPC
  shape在内存中转换后经SSH stdin送入；没有配置/token文件，也未把配置放入env或argv。

任务根：`/home/luoji/code/dashbeam-host-20261115.qxNURz`。
本次独立run目录：`probe-smoke-HaELjL`。

## 既有结果

| # | 方向 | 请求类型 / fixture | 接收阶段ms | 结果 |
| --- | --- | --- | ---: | --- |
| 1 | a→b | legacy / 32KiB | 2450 | 完整接收，内容校验通过 |
| 2 | a→b | legacy / multi，2文件32822字节 | 287 | 完整接收，内容校验通过 |
| 3 | a→b | text / 54字节 | 267 | legacy承载，内容校验通过 |
| 4 | a→b | stream / 空文件 | 30 | 完整接收，ACK成功 |
| 5 | a→b | stream / 32KiB | 31 | 完整接收，ACK成功 |
| 6 | a→b | stream / 8MiB，第1次 | 44 | 完整接收，ACK成功 |
| 7 | a→b | stream / 8MiB，第2次 | 34 | 完整接收，ACK成功 |
| 8 | a→b | stream / 8MiB，第3次 | 39 | 完整接收，ACK成功 |
| 9 | a→b | stream / 64MiB fixture，1ms取消 | 1 | 预期cancelled，输出为空、partial_clean=true |
| 10 | a→b | stream / 取消后32KiB | 31 | 完整接收，ACK成功 |
| 11 | b→a | stream / 32KiB | 30 | 完整接收，ACK成功 |
| 12 | b→a | legacy / 32KiB | 2472 | 完整接收，内容校验通过 |
| 13 | b→a | text / 54字节 | 302 | legacy承载，内容校验通过 |

全部8个stream场景（含预期取消）的`zero_prepare_reads=true`；
7个成功stream场景均确认完成ACK。13项均`node_alive=true`，
每次停止分享后driver也要求控制节点仍存活。取消场景记录`output_entries=0`，
下一次分享继续成功。

表中耗时是接收处理阶段的已有测量，不是点击发送到完整接收的端到端时延。
两个peer都在同一DEV，未强制跨公网relay或真实NAT；不能据此宣称手机、GUI、
功耗、跨中继吞吐或三秒SLO通过。该suite直接驱动native探针，不替代Mac安装版、
邀请前能力协商UI或Android实机验收。

## 收尾与证据

冻结driver在成功路径中已正常forget、quit并等待stopped，再写入最终summary。
随后仅检查现有文件/进程，确认：

- 本run及peer根目录不再有`identity-*`或`stores-*`。
- 本次两个`device_probe --work-dir .../probe-smoke-HaELjL/...`进程均已退出。
- run目录0700，原始`receipts.json`0600，保留的票据文件均0600；未读取或公布票据内容。
- 原始receipts保留于DEV run目录，没有导出到公开报告。

已有日志：`evidence/final-device-probe-smoke.txt`；
退出码：`evidence/final-device-probe-smoke.exit`；
源码预检：`evidence/final-smoke-source-before.txt`。

脱敏结果：本目录`SMOKE-07d999b.results.json`，DEV副本为
`evidence/final-smoke-07d999b.results.json`。仅保留case标签、数值、布尔结果和
清理状态，省略原始网络配置、票据、endpoint、路径遥测及各接收目录。
本次没有修改生产、产品、凭据、系统配置，也没有继续编译或自行扩展验收。
