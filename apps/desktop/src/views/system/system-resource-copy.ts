import { isChineseLocale } from "../../i18n";
import type { ResourceAssessment, ResourceIssue, ResourceState } from "../../domain/system-resources";

export function formatResourceBytes(locale: string, value: number | null) {
  if (value === null || !Number.isFinite(value) || value < 0) return "—";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  const index = value > 0 ? Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1) : 0;
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1, useGrouping: false }).format(value / 1024 ** index)} ${units[index]}`;
}

export function resourceStateLabel(locale: string, state: ResourceState) {
  const labels = isChineseLocale(locale)
    ? { normal: "正常", watch: "需关注", critical: "紧张", unknown: "数据不足", stale: "数据过期" }
    : { normal: "Normal", watch: "Attention", critical: "Critical", unknown: "Incomplete", stale: "Stale" };
  return labels[state];
}

export function resourceUpdatingLabel(locale: string, assessment: ResourceAssessment, refreshing = false) {
  return refreshing && (assessment.state === "stale" || assessment.state === "unknown")
    ? isChineseLocale(locale) ? "更新中…" : "Updating…" : null;
}

export function resourceSampleLabel(locale: string, assessment: ResourceAssessment) {
  if (assessment.sampledAt === null) return isChineseLocale(locale) ? "尚无采样时间" : "No sample time";
  const time = new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit", second: "2-digit" }).format(assessment.sampledAt);
  return isChineseLocale(locale) ? `采样于 ${time}` : `Sampled at ${time}`;
}

function issueDescription(locale: string, issue: ResourceIssue) {
  const zh = isChineseLocale(locale);
  const percent = new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(issue.value);
  switch (issue.code) {
    case "cpu_load": return issue.value < issue.threshold
      ? zh ? `CPU 恢复确认中（${percent}%）` : `CPU recovery pending (${percent}%)`
      : zh ? `CPU 连续采样负载偏高（${percent}%）` : `Repeated high CPU load samples (${percent}%)`;
    case "cpu_core_load": return issue.value < issue.threshold
      ? zh ? `单核恢复确认中（${percent}%）` : `Single-core recovery pending (${percent}%)`
      : zh ? `单核连续采样负载偏高（${percent}%）` : `Repeated high single-core load samples (${percent}%)`;
    case "memory_available": return zh ? `可用物理内存偏低（${formatResourceBytes(locale, issue.value)}）` : `Low available memory (${formatResourceBytes(locale, issue.value)})`;
    case "memory_commit": return zh ? `内存提交余量偏低（${formatResourceBytes(locale, issue.value)}）` : `Low commit headroom (${formatResourceBytes(locale, issue.value)})`;
    case "disk_space": return zh ? `${issue.target || "业务卷"} 可用空间偏低（${formatResourceBytes(locale, issue.value)}）` : `Low free space on ${issue.target || "a storage volume"} (${formatResourceBytes(locale, issue.value)})`;
    case "disk_latency": return issue.value < issue.threshold
      ? zh ? `磁盘延迟恢复确认中（${Math.round(issue.value)} ms）` : `Disk latency recovery pending (${Math.round(issue.value)} ms)`
      : zh ? `磁盘 I/O 延迟连续采样偏高（${Math.round(issue.value)} ms）` : `Repeated high disk I/O latency samples (${Math.round(issue.value)} ms)`;
  }
}

export function resourceMissingLabels(locale: string, missing: string[]) {
  const labels: Record<string, string> = isChineseLocale(locale) ? {
    cpu: "CPU 负载", cpu_cores: "单核负载", memory: "物理内存", memory_commit: "内存提交",
    disk_io: "磁盘 I/O", disk_capacity: "业务卷空间", network: "网络活动"
  } : {
    cpu: "CPU load", cpu_cores: "Single-core load", memory: "Physical memory", memory_commit: "Memory commit",
    disk_io: "Disk I/O", disk_capacity: "Storage volume capacity", network: "Network activity"
  };
  return missing.map((key) => labels[key] || key);
}

export function resourceReason(locale: string, assessment: ResourceAssessment) {
  const zh = isChineseLocale(locale);
  if (assessment.freshness === "stale") {
    return zh ? "采样已过期" : "Sample expired";
  }
  const issue = assessment.issues[0];
  if (issue) return issueDescription(locale, issue);
  if (assessment.state === "unknown") {
    return zh ? "采样不完整" : "Incomplete sample";
  }
  return zh ? "资源正常" : "Resources normal";
}
