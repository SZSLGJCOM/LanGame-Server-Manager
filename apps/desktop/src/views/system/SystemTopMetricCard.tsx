import type { CSSProperties } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import type { SystemCoreInteraction, SystemCoreTone } from "./SystemCoreDial";
import { splitDisplayValue, type TopMetric } from "./useSystemTelemetry";

type MetricTone = SystemCoreTone;

function clampPercent(value: number | null) {
  return value === null || !Number.isFinite(value) ? 0 : Math.max(0, Math.min(100, value));
}

function pointsFromSeries(values: number[], height = 30) {
  if (values.length <= 1) {
    return "";
  }

  return values
    .map((value, index) => {
      const x = (index / (values.length - 1)) * 100;
      const y = height - (clampPercent(value) / 100) * (height - 6) - 3;
      return `${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");
}

function panelStyle(tone?: MetricTone): CSSProperties {
  return tone ? ({ ["--panel-accent" as string]: `var(--system-${tone})` } as CSSProperties) : {};
}

function MiniSparkline({ values, secondaryValues }: { values: number[]; secondaryValues?: number[] }) {
  const primaryPoints = pointsFromSeries(values);
  const secondaryPoints = secondaryValues ? pointsFromSeries(secondaryValues) : "";
  const primaryLastY = values.length > 0
    ? 30 - (clampPercent(values[values.length - 1]) / 100) * 24 - 3
    : null;
  const secondaryLastY = secondaryValues && secondaryValues.length > 0
    ? 30 - (clampPercent(secondaryValues[secondaryValues.length - 1]) / 100) * 24 - 3
    : null;

  return (
    <svg className="system-sparkline" viewBox="0 0 100 30" preserveAspectRatio="none" aria-hidden="true">
      <polyline className="system-sparkline-grid" points="0,22 100,22" />
      <polyline className="system-sparkline-grid" points="0,8 100,8" />
      {primaryPoints ? <polygon className="system-sparkline-area" points={`0,30 ${primaryPoints} 100,30`} /> : null}
      {secondaryPoints ? <polyline className="system-sparkline-line system-sparkline-line--secondary" points={secondaryPoints} /> : null}
      <polyline className="system-sparkline-line" points={primaryPoints} />
      {secondaryLastY !== null ? <circle className="system-sparkline-node system-sparkline-node--secondary" cx="99" cy={secondaryLastY} r="1.2" /> : null}
      {primaryLastY !== null ? <circle className="system-sparkline-node" cx="99" cy={primaryLastY} r="1.35" /> : null}
    </svg>
  );
}

function TopMetricTelemetry({ metric }: { metric: TopMetric }) {
  if (metric.tone === "network") {
    return metric.state === "unknown" || metric.state === "stale" ? null
      : <MiniSparkline values={metric.spark} secondaryValues={metric.secondarySpark} />;
  }
  if (metric.percent === null) return null;
  const loadWidth = `${clampPercent(metric.percent)}%`;

  if (metric.tone === "memory") {
    return (
      <span className="system-top-metric-composition" aria-hidden="true">
        <i style={{ width: loadWidth }} />
        {Array.from({ length: 8 }, (_, index) => <b key={index} />)}
      </span>
    );
  }

  if (metric.tone === "disk") {
    return (
      <span className="system-top-metric-threshold" aria-hidden="true">
        <i style={{ width: loadWidth }} />
      </span>
    );
  }

  return <MiniSparkline values={metric.spark} secondaryValues={metric.secondarySpark} />;
}

interface TopMetricCardProps {
  metric: TopMetric;
  interaction: SystemCoreInteraction;
  linkedLabel: string;
  lockedLabel: string;
}

export function TopMetricCard({ metric, interaction, linkedLabel, lockedLabel }: TopMetricCardProps) {
  const split = splitDisplayValue(metric.value);
  const unit = metric.unit ?? split.unit;
  const active = interaction.activeTone === metric.tone;
  const selected = interaction.pinnedTone === metric.tone;
  const muted = Boolean(interaction.activeTone && !active);
  const style = {
    ...panelStyle(metric.tone),
    ["--metric-load" as string]: `${clampPercent(metric.percent)}%`,
    ["--metric-load-angle" as string]: `${clampPercent(metric.percent) * 3.6}deg`
  } as CSSProperties;

  return (
    <button
      type="button"
      className={[
        "system-hud-panel",
        "system-top-metric",
        `system-top-metric--${metric.tone}`,
        active ? "is-core-active" : "",
        selected ? "is-core-selected" : "",
        muted ? "is-core-muted" : "",
        `is-${metric.state}`
      ].filter(Boolean).join(" ")}
      style={style}
      aria-label={`${metric.title}: ${metric.value}. ${metric.stateLabel}. ${metric.detail}${selected ? `. ${lockedLabel}` : active ? `. ${linkedLabel}` : ""}`}
      aria-pressed={selected}
      onPointerEnter={() => interaction.onHoverChange(metric.tone, true)}
      onPointerLeave={() => interaction.onHoverChange(metric.tone, false)}
      onFocus={(event) => {
        if (event.currentTarget.matches(":focus-visible")) {
          interaction.onFocusChange(metric.tone, true);
        }
      }}
      onBlur={() => interaction.onFocusChange(metric.tone, false)}
      onClick={() => interaction.onPinToggle(metric.tone)}
    >
      {metric.percent !== null ? <span className="system-top-metric-load-rail" aria-hidden="true"><i /></span> : null}
      {active || selected ? (
        <span className="system-top-metric-sync" aria-hidden="true">
          <b>{selected ? lockedLabel : linkedLabel}</b>
        </span>
      ) : null}
      <span className="system-top-metric-head">
        <span className="system-metric-icon">
          <ShellIcon name={metric.icon} />
        </span>
        <span className="system-top-metric-copy">
          <span className="system-top-metric-title" role="heading" aria-level={3}>{metric.title}</span>
          <span className="system-top-metric-channel" title={metric.channel}>{metric.channel}</span>
        </span>
      </span>
      <span className="system-top-metric-value">
        <strong>{split.body}</strong>
        {unit ? <span>{unit}</span> : null}
      </span>
      <span className="system-top-metric-detail" title={metric.detailTitle ?? metric.detail}>{metric.detail}</span>
      <span className="system-top-metric-telemetry">
        <TopMetricTelemetry metric={metric} />
      </span>
    </button>
  );
}
