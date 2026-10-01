import type { ResourceAssessment } from "../../domain/system-resources";
import { isChineseLocale } from "../../i18n";
import { formatResourceBytes, resourceMissingLabels, resourceReason, resourceSampleLabel, resourceUpdatingLabel } from "./system-resource-copy";
import "./system-resource-summary.css";

export function SystemResourceSummary({ assessment, locale, refreshing }: { assessment: ResourceAssessment; locale: string; refreshing?: boolean }) {
  const zh = isChineseLocale(locale);
  const updatingLabel = resourceUpdatingLabel(locale, assessment, refreshing);
  const availableMemory = formatResourceBytes(locale, assessment.memoryAvailableBytes);
  const availableCommit = formatResourceBytes(locale, assessment.memoryCommitAvailableBytes);
  const missingLabels = updatingLabel !== null || assessment.freshness === "stale" ? [] : resourceMissingLabels(locale, assessment.missing);
  const missingDescription = `${zh ? "缺失采样" : "Missing samples"}: ${missingLabels.join(zh ? "、" : ", ")}`;
  const severity = { critical: 2, watch: 1, normal: 0, unknown: -1 };
  const minimumVolume = assessment.volumes.reduce<(typeof assessment.volumes)[number] | null>((minimum, volume) => {
    if (volume.availableBytes === null) return minimum;
    if (minimum?.availableBytes == null || severity[volume.state] > severity[minimum.state]) return volume;
    return severity[volume.state] === severity[minimum.state] && volume.availableBytes < minimum.availableBytes ? volume : minimum;
  }, null);
  const volumeSummary = assessment.volumes.map((volume) =>
    `${volume.label || (zh ? "磁盘" : "Disk")}: ${formatResourceBytes(locale, volume.availableBytes)} ${zh ? "可用" : "free"}`
  ).join("\n");

  return (
    <div className={`system-resource-summary is-${assessment.state}`}>
      <div className="system-resource-sample">
        <p className="system-resource-reason" role="status">
          {updatingLabel ?? resourceReason(locale, assessment)}
          {missingLabels.length > 0 ? <span className="system-resource-missing" title={missingDescription} aria-label={missingDescription}>
            {zh ? ` · 缺失 ${missingLabels.length} 项` : ` · ${missingLabels.length} missing`}
          </span> : null}
        </p>
        <time dateTime={assessment.sampledAt === null ? undefined : new Date(assessment.sampledAt).toISOString()}>
          {resourceSampleLabel(locale, assessment)}
        </time>
      </div>
      <dl className="system-resource-headroom" aria-label={zh ? "资源余量" : "Resource headroom"}>
        <div><dt>{zh ? "内存可用" : "Memory free"}</dt><dd>{availableMemory}</dd></div>
        <div><dt>{zh ? "提交余量" : "Commit free"}</dt><dd>{availableCommit}</dd></div>
        <div className={`is-${minimumVolume?.state ?? "unknown"}`} title={volumeSummary}>
          <dt>{minimumVolume?.label || (zh ? "磁盘" : "Disk")} {zh ? "可用" : "free"}</dt>
          <dd>{formatResourceBytes(locale, minimumVolume?.availableBytes ?? null)}</dd>
        </div>
      </dl>
    </div>
  );
}
