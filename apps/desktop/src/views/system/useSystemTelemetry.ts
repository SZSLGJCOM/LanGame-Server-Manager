import { useEffect, useRef, useState } from "react";
import type { ResourceAssessment, ResourceState } from "../../domain/system-resources";
import { isChineseLocale } from "../../i18n";
import type { SystemSnapshot } from "../../types";
import { CORE_DIAL_SECTORS, type SystemCoreBand, type SystemCoreTone } from "./SystemCoreDial";
import { formatResourceBytes, resourceStateLabel } from "./system-resource-copy";
import { diskHardwareCopy, memoryHardwareCopy } from "./system-hardware-copy";
import { displayVolumePath } from "../../domain/system-volume-label";

export interface TopMetric {
  tone: SystemCoreTone;
  icon: "monitor" | "database" | "shield";
  title: string;
  channel: string;
  detail: string;
  detailTitle?: string;
  value: string;
  unit?: string;
  percent: number | null;
  state: ResourceState;
  stateLabel: string;
  spark: number[];
  secondarySpark?: number[];
}

export function splitDisplayValue(value: string) {
  if (value.endsWith("%")) return { body: value.slice(0, -1), unit: "%" };
  const match = value.match(/^([+-]?\d+(?:[.,]\d+)?)\s+(.+)$/);
  return match ? { body: match[1], unit: match[2] } : { body: value, unit: undefined };
}

type HistoryKey = "cpu" | "receive" | "transmit";
type TelemetryHistory = Record<HistoryKey, number[]>;
type TelemetrySample = Record<HistoryKey, number | null>;
const HISTORY_KEYS: HistoryKey[] = ["cpu", "receive", "transmit"];

function useTelemetryHistory(sample: TelemetrySample, sampledAt: number | null) {
  const sampleRef = useRef(sample);
  sampleRef.current = sample;
  const validity = HISTORY_KEYS.map((key) => sample[key] !== null).join(",");
  const [history, setHistory] = useState<TelemetryHistory>({ cpu: [], receive: [], transmit: [] });
  const previousSample = useRef<number | null>(null);
  useEffect(() => {
    const current = sampleRef.current;
    const isNewSample = sampledAt !== null && previousSample.current !== sampledAt;
    previousSample.current = sampledAt;
    setHistory((previous) => Object.fromEntries(HISTORY_KEYS.map((key) => [
      key,
      current[key] === null ? [] : isNewSample ? [...previous[key], current[key]].slice(-30) : previous[key]
    ])) as TelemetryHistory);
  }, [sampledAt, validity]);
  // Do not display an old line for a channel that has just lost valid telemetry.
  return Object.fromEntries(HISTORY_KEYS.map((key) => [key, sample[key] === null ? [] : history[key]])) as TelemetryHistory;
}

function formatRate(locale: string, value: number | null) {
  return value === null ? "—" : `${formatResourceBytes(locale, value)}/s`;
}

function formatPercent(value: number | null) {
  return value === null ? "—" : `${Math.round(value)}%`;
}

function normalizedVolumeLabel(value: string | null | undefined) {
  return (value ?? "").trim().replace(/^\\\\\?\\/, "").replace(/\//g, "\\").replace(/\\+$/, "").toLowerCase();
}

function channelState(resources: ResourceAssessment, tone: SystemCoreTone, available: boolean): ResourceState {
  if (resources.freshness === "stale") return "stale";
  if (!available) return "unknown";
  const issues = resources.issues.filter((issue) => issue.code.startsWith(tone === "disk" ? "disk_" : `${tone}_`));
  return issues.some((issue) => issue.severity === "critical") ? "critical" : issues.length > 0 ? "watch" : "normal";
}

export function useSystemTelemetry(snapshot: SystemSnapshot, resources: ResourceAssessment, locale: string) {
  const zh = isChineseLocale(locale);
  const unavailable = zh ? "未取得有效采样" : "No valid sample";
  const sampled = zh ? "已采样" : "Sampled";
  const copy = zh ? {
    cpu: "处理器", memory: "内存", disk: "磁盘空间", network: "网络活动",
    total: "总量", free: "可用", cores: "核心", threads: "线程",
    singleCore: "单核峰值", commit: "内存提交", diskLatency: "磁盘 I/O 延迟"
  } : {
    cpu: "Processor", memory: "Memory", disk: "Disk space", network: "Network activity",
    total: "Total", free: "Free", cores: "cores", threads: "threads",
    singleCore: "Single-core peak", commit: "Memory commit", diskLatency: "Disk I/O latency"
  };
  const history = useTelemetryHistory({
    cpu: resources.cpuPercent,
    receive: resources.network.receiveBps,
    transmit: resources.network.transmitBps
  }, resources.sampledAt);
  // Traffic history has its own shared vertical scale; adapter capacity is optional.
  const networkHistoryPeak = Math.max(1, ...history.receive, ...history.transmit);
  const receiveSpark = history.receive.map((value) => value / networkHistoryPeak * 100);
  const transmitSpark = history.transmit.map((value) => value / networkHistoryPeak * 100);
  const current = resources.freshness === "fresh";
  const primaryMatch = snapshot.disk_volume_id
    ? resources.volumes.find((volume) => normalizedVolumeLabel(volume.id) === normalizedVolumeLabel(snapshot.disk_volume_id))
    : snapshot.disk_label ? resources.volumes.find((volume) => normalizedVolumeLabel(volume.label) === normalizedVolumeLabel(snapshot.disk_label)) : undefined;
  const primaryVolume = primaryMatch ?? resources.volumes[0];
  const diskName = diskHardwareCopy(primaryMatch ? snapshot.disk_model : null, primaryVolume?.label, primaryVolume?.paths);
  const diskPercent = current ? primaryVolume?.usedPercent ?? null : null;
  const diskAvailable = current ? primaryVolume?.availableBytes ?? null : null;
  const memoryUsed = resources.memoryTotalBytes !== null && resources.memoryAvailableBytes !== null
    ? resources.memoryTotalBytes - resources.memoryAvailableBytes : null;
  const totalBps = resources.network.receiveBps !== null && resources.network.transmitBps !== null
    ? resources.network.receiveBps + resources.network.transmitBps : null;
  const cpuTopology = [
    current && resources.quality.cpu === "valid" && typeof snapshot.cpu_frequency_mhz === "number" && snapshot.cpu_frequency_mhz > 0
      ? `${new Intl.NumberFormat(locale, { maximumFractionDigits: 2 }).format(snapshot.cpu_frequency_mhz / 1000)} GHz` : null,
    snapshot.cpu_physical_cores ? `${snapshot.cpu_physical_cores} ${copy.cores}` : null,
    snapshot.cpu_logical_cores ? `${snapshot.cpu_logical_cores} ${copy.threads}` : null
  ].filter(Boolean).join(" / ");
  const networkDirection = `RX ${formatPercent(resources.network.receivePercent)} / TX ${formatPercent(resources.network.transmitPercent)}`;
  const onlineAdapters = (Array.isArray(snapshot.network_adapters) ? snapshot.network_adapters : [])
    .filter((adapter) => adapter && typeof adapter.name === "string" && adapter.name
      && typeof adapter.status === "string" && adapter.status.toLowerCase() === "up");
  const networkName = resources.network.adapterName
    ?? onlineAdapters.find((adapter) => !adapter.family_name)?.name ?? onlineAdapters[0]?.name ?? "RX / TX";
  const memoryComposition = memoryUsed === null ? unavailable
    : `${formatResourceBytes(locale, memoryUsed)} / ${formatResourceBytes(locale, resources.memoryTotalBytes)}`;
  const memoryHardware = memoryHardwareCopy(snapshot.memory_modules);
  const memoryDetail = [memoryHardware.speed, memoryComposition].filter(Boolean).join(" · ");
  const baseMetrics = [
    {
      tone: "cpu" as const, icon: "monitor" as const, title: copy.cpu,
      channel: cpuTopology || unavailable, detail: snapshot.cpu_name || unavailable,
      value: formatPercent(resources.cpuPercent), percent: resources.cpuPercent, spark: history.cpu
    },
    {
      tone: "memory" as const, icon: "database" as const, title: copy.memory,
      channel: memoryHardware.model ?? unavailable, detail: memoryDetail,
      detailTitle: `${memoryDetail} · ${copy.free} ${formatResourceBytes(locale, resources.memoryAvailableBytes)}`,
      value: formatPercent(resources.memoryPercent), percent: resources.memoryPercent, spark: []
    },
    {
      tone: "disk" as const, icon: "database" as const, title: copy.disk,
      channel: diskName ?? "—",
      detail: `${copy.free} ${formatResourceBytes(locale, diskAvailable)} / ${copy.total} ${formatResourceBytes(locale, current ? primaryVolume?.totalBytes ?? null : null)}`,
      value: formatPercent(diskPercent), percent: diskPercent, spark: []
    },
    {
      tone: "network" as const, icon: "shield" as const, title: copy.network,
      channel: resources.network.utilizationPercent === null ? networkName
        : `${networkName} · ${networkDirection}`,
      detail: `RX ${formatRate(locale, resources.network.receiveBps)} / TX ${formatRate(locale, resources.network.transmitBps)}`,
      value: formatRate(locale, totalBps), percent: resources.network.utilizationPercent,
      spark: receiveSpark, secondarySpark: transmitSpark
    }
  ];
  const metrics: TopMetric[] = baseMetrics.map((metric) => {
    const state = channelState(resources, metric.tone, metric.tone === "network" ? totalBps !== null : metric.percent !== null);
    return { ...metric, state, stateLabel: state === "normal" ? sampled : resourceStateLabel(locale, state) };
  });
  const coreMainBands: SystemCoreBand[] = metrics.map((metric, index) => {
    const split = splitDisplayValue(metric.value);
    return {
      tone: metric.tone,
      label: zh ? metric.title : ({ cpu: "CPU", memory: "Memory", disk: "Disk", network: "Network" })[metric.tone],
      value: split.body, unit: split.unit,
      detail: metric.detail, micro: metric.channel, percent: metric.percent,
      telemetryState: metric.state,
      telemetryLabel: !zh && metric.state === "unknown" ? "No data" : metric.stateLabel,
      ...CORE_DIAL_SECTORS[index]
    };
  });
  const commitPercent = resources.memoryCommitUsedBytes !== null && resources.memoryCommitLimitBytes !== null && resources.memoryCommitLimitBytes > 0
    ? resources.memoryCommitUsedBytes / resources.memoryCommitLimitBytes * 100 : null;
  const channels = [
    { label: copy.singleCore, value: formatPercent(resources.cpuPeakPercent), percent: resources.cpuPeakPercent,
      detail: cpuTopology || unavailable },
    { label: copy.commit, value: formatPercent(commitPercent), percent: commitPercent,
      detail: `${copy.free} ${formatResourceBytes(locale, resources.memoryCommitAvailableBytes)}` },
    { label: copy.diskLatency, value: resources.diskLatencyMs === null ? "—" : `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(resources.diskLatencyMs)} ms`,
      percent: null, detail: [displayVolumePath(snapshot.disk_label), diskHardwareCopy(snapshot.disk_model, null)].filter(Boolean).join(" · ") || unavailable },
    { label: copy.network, value: formatRate(locale, totalBps), percent: resources.network.utilizationPercent,
      detail: `RX ${formatRate(locale, resources.network.receiveBps)} / TX ${formatRate(locale, resources.network.transmitBps)}` }
  ];
  const coreChannelBands: SystemCoreBand[] = channels.map((channel, index) => ({
    ...coreMainBands[index], ...channel, ...splitBandValue(channel.value),
    telemetryState: channel.value === "—" ? (current ? "unknown" : resources.freshness === "stale" ? "stale" : "unknown") : coreMainBands[index].telemetryState,
    telemetryLabel: channel.value === "—" ? unavailable : coreMainBands[index].telemetryLabel
  }));
  return { metrics, coreMainBands, coreChannelBands };
}

function splitBandValue(value: string) {
  const split = splitDisplayValue(value);
  return { value: split.body, unit: split.unit };
}
