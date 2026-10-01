import type { TranslateFn } from "./i18n";
import { isChineseLocale } from "./i18n-config";
import type { RuntimeHealthSummary } from "./types";

const reasonMessageKeys: Record<string, string> = {
  log_read_failed: "runtime.health.logReadFailed",
  stopped: "runtime.health.stopped",
  stopping: "runtime.health.stopping",
  not_available: "runtime.health.notAvailable",
  dst_lua_config_failed: "runtime.health.dstLuaConfigFailed",
  fatal_log_pattern: "runtime.health.fatalLogPattern",
  ready_signal: "runtime.health.readySignal",
  latest_run_failed: "runtime.health.latestRunFailed",
  starting_waiting_logs: "runtime.health.startingWaitingLogs",
  starting_tasks: "runtime.health.startingTasks",
  retry_signal: "runtime.health.retrySignal",
  abiotic_world_corrupt: "runtime.health.abioticWorldCorrupt",
  abiotic_session_published: "runtime.health.abioticSessionPublished",
  abiotic_loading_map: "runtime.health.abioticLoadingMap",
  abiotic_listening: "runtime.health.abioticListening",
  abiotic_validating_world: "runtime.health.abioticValidatingWorld",
  dragonwilds_ready_map: "runtime.health.dragonwildsReadyMap",
  dragonwilds_ready: "runtime.health.dragonwildsReady",
  dragonwilds_owner_invalid: "runtime.health.dragonwildsOwnerInvalid",
  ark_udp_ready: "runtime.health.arkUdpReady",
  ark_udp_pending: "runtime.health.arkUdpPending",
  astroneer_world_query_unavailable: "runtime.health.astroneerWorldQueryUnavailable",
  astroneer_console_configuration: "runtime.health.astroneerConsoleConfiguration"
};

const chineseReasons: Record<string, string> = {
  log_read_failed: "无法读取运行日志：{error}",
  stopped: "服务器当前已停止。",
  stopping: "服务器正在停止。",
  not_available: "暂无运行状态摘要。",
  dst_lua_config_failed: "饥荒联机版正在运行，但分片 Lua 配置文件加载失败。请保存实例配置以重新生成集群文件。",
  fatal_log_pattern: "最近的日志中检测到致命运行错误。",
  ready_signal: "服务器已报告就绪或正在监听连接。",
  latest_run_failed: "最近一次运行以错误状态结束。",
  starting_waiting_logs: "进程正在运行，等待启动日志输出。",
  starting_tasks: "进程正在运行，仍在执行启动任务。",
  retry_signal: "服务器正在运行，但最近的日志中出现了警告或重试信号。",
  abiotic_world_corrupt: "非生物因素检测到世界存档损坏，专用服务器无法安全地继续运行。",
  abiotic_session_published: "非生物因素已上线，并发布了供玩家加入的会话短码。",
  abiotic_loading_map: "非生物因素已完成世界校验，正在加载主设施地图。",
  abiotic_listening: "非生物因素正在监听连接并准备游戏会话。",
  abiotic_validating_world: "非生物因素正在校验所选世界存档并准备地图。",
  dragonwilds_ready_map: "RuneScape: Dragonwilds 已在地图 {map} 创建 GameSession，并报告 ReadyToJoin。",
  dragonwilds_ready: "RuneScape: Dragonwilds 已创建 GameSession，并报告 ReadyToJoin。",
  dragonwilds_owner_invalid: "RuneScape: Dragonwilds 因 OwnerId 缺失或无效，未通过 DedicatedServer.ini 校验。",
  ark_udp_ready: "方舟：生存进化已绑定所需 UDP 端口（{ports}），可以接收玩家。",
  ark_udp_pending: "方舟：生存进化已绑定 UDP 端口 {bound}，仍在等待端口 {missing}。",
  astroneer_world_query_unavailable: "无法从当前异星探险家服务器取得完整的原生世界状态。",
  astroneer_console_configuration: "异星探险家的世界状态查询需要一个控制台 TCP 端口和有效的控制台密码。"
};

export function localizeRuntimeHealthSummary(health: RuntimeHealthSummary, locale: string, t?: TranslateFn): string {
  const key = health.reason ? reasonMessageKeys[health.reason.code] : undefined;
  if (!key || !health.reason) return health.summary;
  const translated = t?.(key, health.reason.params);
  if (translated && translated !== key) return translated;
  const template = isChineseLocale(locale) ? chineseReasons[health.reason.code] : undefined;
  if (!template) return health.summary;
  return template.replace(/\{\s*([\w.]+)\s*\}/g, (match, parameter: string) => health.reason?.params[parameter] ?? match);
}
