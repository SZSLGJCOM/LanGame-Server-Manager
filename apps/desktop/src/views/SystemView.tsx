import { CSSProperties, ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { isChineseLocale, selectLocaleText, useI18n } from "../i18n";
import { useInstanceConnections } from "../hooks/useInstanceConnections";
import { readPreferredJoinAddress, resolveInstanceConnection } from "../instance-connections";
import type {
  AppSettings,
  BindAddressCandidate,
  InstanceSummary,
  SteamCmdStatus,
  SteamCmdPrepareSnapshot,
  SystemSnapshot
} from "../types";
import { countRunningInstances, formatInstanceStatus, valheimCrossplayJoinGuidance } from "../view-models";
import { resolveSystemPlayerCounts } from "../system-player-counts";
import {
  SystemCoreDial,
  type SystemCoreInteraction,
  type SystemCoreTone
} from "./system/SystemCoreDial";
import { useSystemResources } from "../hooks/useSystemResources";
import { useSystemTelemetry } from "./system/useSystemTelemetry";
import { TopMetricCard } from "./system/SystemTopMetricCard";
import { SystemResourceSummary } from "./system/SystemResourceSummary";
import { resourceSampleLabel, resourceStateLabel, resourceUpdatingLabel } from "./system/system-resource-copy";
import type { StorageMigrationModel } from "../hooks/useStorageMigration";
import { StoragePathsPanel } from "./system/StoragePathsPanel";
import "./SystemView.css";

interface SystemViewProps {
  snapshot: SystemSnapshot;
  systemRefreshing?: boolean;
  instances: InstanceSummary[];
  bindAddressCandidates: BindAddressCandidate[];
  appSettings: AppSettings;
  steamCmdStatus: SteamCmdStatus | null;
  steamCmdBusy: boolean;
  steamCmdProgress: SteamCmdPrepareSnapshot | null;
  steamCmdMessage: string;
  onOpenInstance: (instanceId: string) => void;
  onPickDirectory: (currentPath?: string | null) => Promise<string | null>;
  storageMigration: StorageMigrationModel;
  onEnsureSteamCmd: () => void;
  onUninstallSteamCmd: () => void;
}

type MetricTone = SystemCoreTone;

function formatInteger(locale: string, value: number) {
  return new Intl.NumberFormat(locale).format(value);
}

function panelStyle(tone?: MetricTone): CSSProperties {
  return tone ? ({ ["--panel-accent" as string]: `var(--system-${tone})` } as CSSProperties) : {};
}

function normalizedInstanceStatus(status: string) {
  return status.trim().toLowerCase().replace(/[^a-z0-9_-]+/g, "-") || "unknown";
}

function instanceOverviewPriority(status: string) {
  switch (normalizedInstanceStatus(status)) {
    case "error":
      return 0;
    case "running":
    case "starting":
    case "stopping":
      return 1;
    default:
      return 2;
  }
}

function SystemPanel({
  className,
  tone,
  children
}: {
  className?: string;
  tone?: MetricTone;
  children: ReactNode;
}) {
  return (
    <section className={["system-hud-panel", className].filter(Boolean).join(" ")} style={panelStyle(tone)}>
      {children}
    </section>
  );
}

function PanelHeader({
  title,
  eyebrow,
  aside
}: {
  title: string;
  eyebrow: string;
  aside?: ReactNode;
}) {
  return (
    <div className="system-panel-head">
      <div>
        <h2>{title}</h2>
        <span>{eyebrow}</span>
      </div>
      {aside ? <div className="system-panel-aside">{aside}</div> : null}
    </div>
  );
}

function SystemDashboard(props: SystemViewProps) {
  const { locale, t } = useI18n();
  const instanceConnections = useInstanceConnections(props.instances, null);
  const [coreHoveredTone, setCoreHoveredTone] = useState<MetricTone | null>(null);
  const [coreFocusedTone, setCoreFocusedTone] = useState<MetricTone | null>(null);
  const [corePinnedTone, setCorePinnedTone] = useState<MetricTone | null>(null);
  const coreActiveTone = corePinnedTone ?? coreFocusedTone ?? coreHoveredTone;
  const handleCoreHoverChange = useCallback((tone: MetricTone, active: boolean) => {
    setCoreHoveredTone((current) => active ? tone : current === tone ? null : current);
  }, []);
  const handleCoreFocusChange = useCallback((tone: MetricTone, active: boolean) => {
    setCoreFocusedTone((current) => active ? tone : current === tone ? null : current);
  }, []);
  const handleCorePinToggle = useCallback((tone: MetricTone) => {
    setCorePinnedTone((current) => current === tone ? null : tone);
  }, []);
  useEffect(() => {
    const clearCorePin = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setCorePinnedTone(null);
      }
    };

    window.addEventListener("keydown", clearCorePin);
    return () => window.removeEventListener("keydown", clearCorePin);
  }, []);
  const coreInteraction = useMemo<SystemCoreInteraction>(() => ({
    activeTone: coreActiveTone,
    pinnedTone: corePinnedTone,
    onHoverChange: handleCoreHoverChange,
    onFocusChange: handleCoreFocusChange,
    onPinToggle: handleCorePinToggle
  }), [coreActiveTone, corePinnedTone, handleCoreFocusChange, handleCoreHoverChange, handleCorePinToggle]);
  const useChineseCopy = isChineseLocale(locale);
  const resources = useSystemResources(props.snapshot);
  const updatingLabel = resourceUpdatingLabel(locale, resources, props.systemRefreshing);
  const { metrics, coreMainBands, coreChannelBands } = useSystemTelemetry(props.snapshot, resources, locale);
  const copy = useChineseCopy ? {
    coreTitle: "系统核心", coreMeta: "主机资源采样", resource: "主机资源",
    coreLinked: "联动", coreLocked: "锁定", instance: "实例概览", instanceMeta: "运行状态与快捷访问",
    running: "运行", players: "人数", playersUnknown: "未知", playersPartial: "部分数据",
    playerQueryCoverage: "人数查询覆盖", abnormal: "异常", autostart: "自启动", autostartBadge: "自启",
    openInstance: "打开实例", emptyInstances: "暂无托管实例",
    emptyInstancesHint: "从游戏库选择一款游戏，创建你的第一个服务器。"
  } : {
    coreTitle: "System Core", coreMeta: "Host resource samples", resource: "Resources",
    coreLinked: "SYNC", coreLocked: "LOCKED", instance: "Instance Overview", instanceMeta: "Status and quick access",
    running: "Running", players: "Players", playersUnknown: "Unknown", playersPartial: "Partial data",
    playerQueryCoverage: "Player queries", abnormal: "Errors", autostart: "Autostart", autostartBadge: "AUTO",
    openInstance: "Open instance", emptyInstances: "No managed instances",
    emptyInstancesHint: "Choose a game in the library to create your first server."
  };
  const totalInstances = props.instances.length;
  const playerCounts = resolveSystemPlayerCounts(props.snapshot, countRunningInstances(props.instances));
  const runningCount = playerCounts.runningInstances;
  const abnormalCount = props.instances.filter((instance) => normalizedInstanceStatus(instance.status) === "error").length;
  const autostartCount = props.instances.filter((instance) => instance.autostart).length;
  const orderedInstances = [...props.instances].sort((left, right) =>
    instanceOverviewPriority(left.status) - instanceOverviewPriority(right.status)
    || left.name.localeCompare(right.name, locale)
  );
  const playerCountValue = playerCounts.onlinePlayers === null
    ? "—"
    : `${playerCounts.state === "partial" ? "≥" : ""}${formatInteger(locale, playerCounts.onlinePlayers)}`;
  const playerCountDisplay = playerCounts.capacity === null
    ? playerCountValue
    : `${playerCountValue} / ${formatInteger(locale, playerCounts.capacity)}`;
  const playerCountNote = playerCounts.state === "unknown" ? copy.playersUnknown
    : playerCounts.state === "partial" ? copy.playersPartial : null;
  const playerCoverage = `${copy.playerQueryCoverage} ${playerCounts.queriedInstances === null
    ? "—" : formatInteger(locale, playerCounts.queriedInstances)} / ${formatInteger(locale, runningCount)}`;
  return (
    <div className="page-grid workspace-page system-dashboard-page">
      <div className="system-command-grid">
        <SystemPanel className="system-core-panel">
          <PanelHeader
            title={copy.coreTitle}
            eyebrow={copy.coreMeta}
          />
          <div className="system-core-stage" aria-busy={updatingLabel !== null}>
            <SystemCoreDial
              resourceLabel={copy.resource}
              stateLabel={updatingLabel ?? resourceStateLabel(locale, resources.state)}
              hideIdleSampleState={updatingLabel !== null}
              sampleLabel={resourceSampleLabel(locale, resources)}
              sampleStateLabel={resources.freshness === "fresh"
                ? selectLocaleText(locale, "已采样", "Sampled")
                : resourceStateLabel(locale, resources.state)}
              lockedLabel={copy.coreLocked}
              rimCaption={copy.coreMeta}
              operatingState={resources.state}
              mainBands={coreMainBands}
              channelBands={coreChannelBands}
              interaction={coreInteraction}
            />
          </div>
          <SystemResourceSummary assessment={resources} locale={locale} refreshing={props.systemRefreshing} />
        </SystemPanel>

        {metrics.map((metric) => (
          <TopMetricCard
            key={metric.tone}
            metric={metric}
            interaction={coreInteraction}
            linkedLabel={copy.coreLinked}
            lockedLabel={copy.coreLocked}
          />
        ))}

        <SystemPanel className="system-instance-panel" tone="cpu">
          <PanelHeader
            title={copy.instance}
            eyebrow={copy.instanceMeta}
            aside={
              <div className="system-instance-summary-bar">
                <div className="system-instance-summary-item">
                  <span>{copy.running}</span>
                  <strong>{runningCount} / {totalInstances}</strong>
                </div>
                <div className="system-instance-summary-item">
                  <span>{playerCountNote ? `${copy.players} · ${playerCountNote}` : copy.players}</span>
                  <strong title={playerCoverage}>{playerCountDisplay}</strong>
                </div>
                <div className="system-instance-summary-item">
                  <span>{copy.abnormal}</span>
                  <strong>{abnormalCount}</strong>
                </div>
                <div className="system-instance-summary-item">
                  <span>{copy.autostart}</span>
                  <strong>{autostartCount}</strong>
                </div>
              </div>
            }
          />

          <div className="system-instance-list">
            {props.instances.length === 0 ? (
              <div className="system-instance-empty">
                <strong>{copy.emptyInstances}</strong>
                <span>{copy.emptyInstancesHint}</span>
              </div>
            ) : (
              orderedInstances.map((instance) => {
                const gameAbbr = (instance.module_id || "game")
                  .substring(0, 3)
                  .toUpperCase();
                const normalizedStatus = normalizedInstanceStatus(instance.status);
                const statusLabel = formatInstanceStatus(instance.status, t);
                const connection = instanceConnections.connections[instance.id];
                const connectionDetails = connection ? {
                  summary: { ...instance, bind_ip: connection.bind_ip },
                  ports: connection.ports,
                  settings_json: connection.settings_json
                } : null;
                const crossplay = valheimCrossplayJoinGuidance(connectionDetails, t);
                const endpoint = connectionDetails ? resolveInstanceConnection(connectionDetails,
                  props.bindAddressCandidates, readPreferredJoinAddress(instance.id), locale, t) : null;
                const connectionLabel = crossplay?.summary ?? endpoint?.endpoint ?? (instanceConnections.failed
                  ? selectLocaleText(locale, "连接信息读取失败", "Connection unavailable")
                  : connection ? selectLocaleText(locale, "暂无可用连接地址", "No join address available")
                    : selectLocaleText(locale, "正在读取连接信息…", "Loading connection…"));

                return (
                  <button
                    key={instance.id}
                    type="button"
                    className={`system-instance-item status-${normalizedStatus}`}
                    aria-label={`${copy.openInstance}: ${instance.name}, ${statusLabel}`}
                    onClick={() => props.onOpenInstance(instance.id)}
                  >
                    <div className="system-instance-item-icon" data-game={instance.module_id}>
                      {gameAbbr}
                    </div>
                    <div className="system-instance-item-info">
                      <div className="system-instance-item-name-row">
                        <span className="instance-name">{instance.name}</span>
                        {instance.autostart && <span className="instance-autostart-badge" title={copy.autostart}>{copy.autostartBadge}</span>}
                      </div>
                      <div className="system-instance-item-address"
                        title={crossplay?.instructions ?? (endpoint ? `${endpoint.label} · ${endpoint.endpoint}` : connectionLabel)}>
                        {connectionLabel}
                      </div>
                    </div>
                    <div className="system-instance-item-status">
                      <span className={`status-pill pill-${normalizedStatus}`}>
                        {statusLabel}
                      </span>
                    </div>
                  </button>
                );
              })
            )}
          </div>
        </SystemPanel>

        <StoragePathsPanel
          appSettings={props.appSettings}
          steamCmdStatus={props.steamCmdStatus}
          steamCmdBusy={props.steamCmdBusy}
          steamCmdProgress={props.steamCmdProgress}
          steamCmdMessage={props.steamCmdMessage}
          onPickDirectory={props.onPickDirectory}
          storageMigration={props.storageMigration}
          onEnsureSteamCmd={props.onEnsureSteamCmd}
          onUninstallSteamCmd={props.onUninstallSteamCmd}
        />
      </div>
    </div>
  );
}

export function SystemView(props: SystemViewProps) {
  return <SystemDashboard {...props} />;
}
