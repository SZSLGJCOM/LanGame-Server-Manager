# System resource monitoring

[English](#english) | [简体中文](#简体中文)

## English

The home page's System Core reports **host resource conditions**, not game availability,
security assurance, or how many additional servers can start. Instance status and player
query coverage remain separate. Normal means no pressure was identified in the available,
required observations; it does not guarantee that every game or player connection works.

### Evidence and freshness

Each host observation carries its collection start time and per-channel quality:
`valid`, `warming_up`, or `unavailable`. Initial CPU/network rate samples require a
previous counter reading. Missing or failed collection must not be interpreted as zero
usage. Cached results retain their original observation time.
Metadata reloads retain the last host observation instead of replacing it with the
initial empty snapshot. Server-panel failures do not pause home-page sampling.
Cold counters receive one additional request after the native 15-second cache expires.
Entering System starts a request immediately. Navigation and foreground/background
changes share a pending request rather than discarding its result and starting another.
Concurrent native requests wait for the same collector, with a 20-second deadline per
request. An expired cached reply receives at most one immediate follow-up; the collector
continues independently of request deadlines. While waiting on stale or incomplete
readings, the existing status shows Updating and retains the original sample time.
Fresh readings remain visible; completion or failure clears the updating state.

Development source synchronization and frontend hot reload do not replace a running
native runtime. After a telemetry contract change, fully exit from the tray on the
affected workstation, wait for the normal save/stop workflow, and run `start.bat`
there to rebuild and start the updated backend. Closing the window only hides it;
launching again while the runtime is alive reuses that runtime.

The existing home-page polling runs every 60 seconds in the foreground and 120 seconds
when throttled. An observation expires after 180 seconds, including when polling is
paused or fails. The interface checks age independently of successful responses, hides
expired readings, and retains the last collection time. This is sampled monitoring,
not continuous or real-time incident detection.

### Resource policy

These are explicit, conservative attention thresholds, not universal game requirements
or a weighted health score. Known critical pressure takes precedence over incomplete
coverage; missing measurements are still disclosed.

| Signal | Attention | Critical / recovery |
| --- | --- | --- |
| Total CPU | At least 90% in two observations | Clear after two observations below 80% |
| Busiest logical CPU | At least 95% in two observations | Clear after two observations below 85% |
| Selected volume read/write latency | Maximum at least 20 ms in two observations | Clear after two observations below 15 ms |
| Available physical memory | At most 2 GiB **and** 10% of physical memory | Critical at most 1 GiB **and** 5% |
| System commit headroom | At most 2 GiB **and** 10% of the commit limit | Critical at most 512 MiB **and** 5% |
| Available space on any monitored volume | At most 5 GiB, or at most 20 GiB **and** 5% of volume capacity | Critical at most 1 GiB |

CPU and I/O confirmation uses distinct observations separated by at least 30 seconds.
Rerenders and repeated cached results do not count as new samples. Missing readings or
gaps of 180 seconds reset confirmation. Two point samples are evidence of repeated
pressure, not proof of uninterrupted saturation. Low memory, commit headroom and disk
capacity are reported immediately. These checks do not replace a game's installation,
update, backup or startup capacity requirements.

Volume capacity covers configured game, instance, archive and SteamCMD roots plus
registered instance storage paths, grouped by actual volume without recursively scanning
files. A failed path/volume query leaves coverage incomplete. The selected volume's I/O
measurement is separate from capacity checks across all volumes.

Hardware labels stay in the existing metric rows. Memory uses the reported manufacturer,
part number and configured transfer rate (MT/s), falling back to reported module speed.
Disk model follows the selected volume's physical disk extents and is cached for ten
minutes; changing the selected volume refreshes it. Missing models fall back to readable
mount paths, never internal volume GUIDs. Capacity and model use the same volume identity.

Network traffic does not change resource state. Measured host RX/TX rates and their
history remain available without a reported adapter capacity. Both history lines share
a vertical scale based on their recent peak. Selecting the network channel shows its
measured throughput. When an online, non-overlay adapter
reports its link rate, RX and TX utilization each use that adapter's own byte rate and
reported bits-per-second rate. The displayed adapter is the one with the highest
directional utilization. Full-duplex directions are not added, and host-wide byte rates
are not divided by one adapter's capacity. A reported link rate is not an ISP bandwidth
limit or a measurement of latency, loss, or player reachability.

### Verification

The resource model is in `apps/desktop/src/domain/system-resources.ts`. Behavioral
regressions cover missing/invalid/expired observations, independent memory constraints,
secondary-volume exhaustion, sample confirmation/recovery, and network denominators.
The browser fixture verifies the real home-page components using explicitly synthetic
observations, including keyboard interaction and both interface languages.

## 简体中文

首页系统核心展示**主机资源状态**，不代表游戏可用性、安全保证或还能启动多少实例。
实例状态与人数查询覆盖单独展示。“正常”表示当前必需的有效观测未发现资源压力，
不保证所有游戏和玩家连接正常。

每次观测保留采集开始时间，各通道区分有效、预热和不可用。CPU与网络差分速率的
首次观测需要建立基线；失败或缺失数据不能当作零占用。缓存不会更新原始观测时间。
普通页面刷新保留最近一次硬件观测，不用初始空快照覆盖；服务器面板读取失败不会暂停首页采样。
首次差分计数预热时，会在原生15秒缓存到期后补采样一次。
进入系统页立即请求；切页及前后台切换复用尚未完成的请求，不丢弃结果后另启采集。
原生并发请求等待同一次采集，每次请求最多等待20秒；过期回包最多立即追加一次请求，
采集任务不因请求等待到期而取消。过期或不完整数据刷新时，原状态位置显示“更新中”，
保留真实采样时间；新鲜读数继续显示，请求完成或失败后结束更新提示。
开发源码同步及前端热更新不会替换正在运行的原生后台。采样接口变更后，须在受影响电脑
从托盘完整退出，等待正常保存与停服流程完成，再运行该电脑的 `start.bat` 编译启动新版后台。
关闭窗口仅隐藏界面；后台仍运行时重复启动会复用原服务。
首页沿用前台60秒、节流120秒的采样请求间隔；180秒后数据过期，即使刷新暂停或
请求失败也会独立过期。界面隐藏过期读数并保留最后采集时间，不宣称连续实时监测。

资源提醒采用可解释的默认阈值，不计算加权健康分：

- 总CPU连续两次观测达到90%、最忙逻辑核达到95%、目标卷读写延迟最大值达到20ms时提醒；
  分别连续两次低于80%、85%、15ms后恢复。观测间隔至少30秒；同一缓存、重复渲染不计数。
  缺失采样或180秒以上间隔会清除连续确认。两次采样偏高不等于证明全程饱和。
- 物理可用内存同时不超过2GiB及总量10%时提醒，同时不超过1GiB及5%时标记紧张。
- 系统提交余量同时不超过2GiB及提交上限10%时提醒，同时不超过512MiB及5%时标记紧张。
- 任一受监控卷可用空间不超过5GiB，或同时不超过20GiB及卷容量5%时提醒；
  不超过1GiB时标记紧张。低容量立即报告，不等待连续确认。

这些是资源关注策略，不是所有游戏通用的性能标准。确定的紧张资源优先显示，其他
缺失数据仍需披露；安装、更新、备份与开服仍应检查各自实际需求。

容量覆盖配置的游戏、实例、归档、SteamCMD根目录和已登记实例的存储路径，按实际卷
归并，不递归扫描文件。查询失败不能算作完整覆盖。目标卷I/O与所有业务卷容量分别表达。

硬件信息沿用原指标行：内存显示系统报告的厂商、型号和配置速率（MT/s），配置速率缺失时
使用模块报告速率。磁盘型号按目标卷的物理盘映射获取，缓存十分钟，切换目标卷时刷新。
型号缺失时显示可读挂载路径，不展示内部卷GUID；容量和型号按同一卷标识关联。

网络活动按实际收发速率展示；未取得网卡链路容量时仍显示有效吞吐和历史曲线，
收发曲线使用近期峰值作为共同纵轴，选中网络通道显示吞吐。有效在线非覆盖网络网卡的收发占比分别按其报告链路速率计算，
选择方向占用最高的网卡并标明名称。全双工收发不相加，主机总流量也不混用单网卡分母。
网卡报告速率不代表运营商带宽、延迟、丢包或玩家可达性，网络吞吐不参与资源状态判断。

行为回归测试覆盖未知、无效、过期、提交内存压力、其他业务卷满盘及确认/恢复逻辑；
浏览器验收使用明确的合成快照渲染真实首页，检查键盘操作、中英文和布局。
