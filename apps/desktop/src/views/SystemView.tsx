import { ActivityNotice } from "../components/ActivityNotice";
import { CSSProperties, ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { openLocalPath } from "../api";
import { ShellIcon } from "../components/ShellIcon";
import { isChineseLocale, selectLocaleText, useI18n } from "../i18n";
import { useInstanceConnections } from "../hooks/useInstanceConnections";
import { readPreferredJoinAddress, resolveInstanceConnection } from "../instance-connections";
import type {
  AppPathSettingsInput,
  AppSettings,
  BindAddressCandidate,
  InstanceSummary,
  SteamCmdStatus,
  SteamCmdPrepareSnapshot,
  SystemSnapshot
} from "../types";
import { countRunningInstances, formatInstanceStatus } from "../view-models";
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
  onSaveAppSettings: (settings: AppPathSettingsInput) => void | Promise<void>;
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

interface DefaultPathsPanelProps {
  appSettings: AppSettings;
  steamCmdStatus: SteamCmdStatus | null;
  steamCmdBusy: boolean;
  steamCmdProgress: SteamCmdPrepareSnapshot | null;
  steamCmdMessage: string;
  onPickDirectory: (currentPath?: string | null) => Promise<string | null>;
  onSaveAppSettings: (settings: AppPathSettingsInput) => void | Promise<void>;
  onEnsureSteamCmd: () => void;
  onUninstallSteamCmd: () => void;
}

function DefaultPathsPanel(props: DefaultPathsPanelProps) {
  const { locale, t } = useI18n();
  const [pathBusyKey, setPathBusyKey] = useState<keyof AppPathSettingsInput | null>(null);
  const [pathAction, setPathAction] = useState<"choose" | "open" | "copy" | null>(null);
  const [pathFeedback, setPathFeedback] = useState<{ message: string; error: boolean } | null>(null);
  const executablePath = props.steamCmdStatus?.executable_path || t("system.runtimeUnknownPath");
  const instancesRoot = props.appSettings.servers_root.trim();
  // Match storage's default when no separate archive directory has been set.
  const archivesRoot = props.appSettings.archives_root?.trim()
    || `${instancesRoot.replace(/[\\/]+$/, "")}${instancesRoot.includes("\\") ? "\\" : "/"}.trash`;
  const copy = isChineseLocale(locale)
    ? {
        title: "存储与运行环境",
        eyebrow: "服务器文件、实例与 SteamCMD",
        gamesRoot: "服务器文件",
        serversRoot: "实例工作区",
        archivesRoot: "实例归档区",
        steamCmdRoot: "SteamCMD",
        browse: "选择",
        browsing: "选择中",
        copied: "路径已复制",
        opened: "文件夹已打开",
        saved: "默认路径已更新",
        failed: "路径操作失败"
      }
    : {
        title: "Storage & runtime",
        eyebrow: "Server files, instances and SteamCMD",
        gamesRoot: "Server files",
        serversRoot: "Instance workspace",
        archivesRoot: "Instance archives",
        steamCmdRoot: "SteamCMD",
        browse: "Choose",
        browsing: "Choosing",
        copied: "Path copied",
        opened: "Folder opened",
        saved: "Default path updated",
        failed: "Path action failed"
      };
  const pathEntries = [
    {
      key: "games_root" as const,
      icon: "package" as const,
      label: copy.gamesRoot,
      value: props.appSettings.games_root
    },
    {
      key: "servers_root" as const,
      icon: "database" as const,
      label: copy.serversRoot,
      value: props.appSettings.servers_root
    },
    {
      key: "archives_root" as const,
      icon: "inbox" as const,
      label: copy.archivesRoot,
      value: archivesRoot
    },
    {
      key: "steamcmd_root" as const,
      icon: "terminal" as const,
      label: copy.steamCmdRoot,
      value: props.appSettings.steamcmd_root
    }
  ];

  const showPathError = (error: unknown) => {
    const detail = error instanceof Error ? error.message : String(error);
    setPathFeedback({ message: `${copy.failed}: ${detail}`, error: true });
  };

  async function handlePathAction(entry: typeof pathEntries[number], action: "open" | "copy") {
    setPathBusyKey(entry.key);
    setPathAction(action);
    setPathFeedback(null);
    try {
      if (action === "open") {
        await openLocalPath(entry.value);
      } else {
        await navigator.clipboard.writeText(entry.value);
      }
      setPathFeedback({ message: `${action === "open" ? copy.opened : copy.copied}: ${entry.label}`, error: false });
    } catch (error) {
      showPathError(error);
    } finally {
      setPathBusyKey(null);
      setPathAction(null);
    }
  }

  async function handlePickPath(key: keyof AppPathSettingsInput) {
    setPathBusyKey(key);
    setPathAction("choose");
    setPathFeedback(null);
    try {
      const currentSettings: AppPathSettingsInput = {
        games_root: props.appSettings.games_root,
        servers_root: props.appSettings.servers_root,
        archives_root: archivesRoot,
        steamcmd_root: props.appSettings.steamcmd_root
      };
      const selected = await props.onPickDirectory(currentSettings[key]);
      const nextValue = selected?.trim();
      if (!nextValue || nextValue === currentSettings[key].trim()) {
        return;
      }

      await props.onSaveAppSettings({
        ...currentSettings,
        [key]: nextValue
      });
      setPathFeedback({ message: copy.saved, error: false });
    } catch (error) {
      showPathError(error);
    } finally {
      setPathBusyKey(null);
      setPathAction(null);
    }
  }

  const btnStatusTone = props.steamCmdBusy
    ? "status-busy"
    : props.steamCmdProgress?.error
      ? "status-danger"
      : props.steamCmdStatus?.ready
        ? "status-success"
        : props.steamCmdStatus?.ownership === "invalid"
          ? "status-danger"
          : "status-warning";

  return (
    <SystemPanel className="system-runtime-panel">
      <PanelHeader
        title={copy.title}
        eyebrow={copy.eyebrow}
        aside={(
          <div className="system-runtime-header-actions">
            {props.steamCmdStatus?.can_uninstall && (
              <button
                type="button"
                className="danger"
                onClick={props.onUninstallSteamCmd}
                disabled={props.steamCmdBusy || pathBusyKey !== null}
              >
                <span className="status-dot" aria-hidden="true" />
                <span>{t("system.runtimeUninstallSteamCmd", undefined, "Uninstall")}</span>
              </button>
            )}
            <button
              type="button"
              className={`system-runtime-check-button ${btnStatusTone}`.trim()}
              title={[props.steamCmdMessage, executablePath].filter(Boolean).join("\n") || undefined}
              aria-live="polite"
              onClick={props.onEnsureSteamCmd}
              disabled={props.steamCmdBusy || pathBusyKey !== null}
            >
              <span className="status-dot" aria-hidden="true" />
              <span>{props.steamCmdBusy
                ? props.steamCmdProgress ? t(`steamcmd.prepare.${props.steamCmdProgress.phase}`) : t("system.runtimeChecking")
                : props.steamCmdProgress?.error ? t("steamcmd.retry") : t("system.runtimeCheckSteamCmd")}</span>
            </button>
          </div>
        )}
      />
      <div className="system-paths-content">
        <div className="system-path-list">
          {pathEntries.map((entry) => (
            <div key={entry.key} className="system-path-row" data-path-key={entry.key}>
              <span className="system-path-row-icon" aria-hidden="true">
                <ShellIcon name={entry.icon} />
              </span>
              <div className="system-path-row-copy">
                <span className="system-runtime-label">{entry.label}</span>
                <div className="system-runtime-path" title={entry.value}>{entry.value}</div>
              </div>
              <div className="system-runtime-path-actions">
                <button
                  type="button"
                  className="system-runtime-path-action-btn"
                  title={t("system.runtimeCopyPath", undefined, "Copy path")}
                  aria-label={`${t("system.runtimeCopyPath", undefined, "Copy path")}: ${entry.label}`}
                  disabled={pathBusyKey !== null || props.steamCmdBusy}
                  onClick={() => void handlePathAction(entry, "copy")}
                >
                  <ShellIcon name="copy" />
                </button>
                <button
                  type="button"
                  className="system-runtime-path-action-btn"
                  title={t("system.runtimeOpenFolder", undefined, "Open folder")}
                  aria-label={`${t("system.runtimeOpenFolder", undefined, "Open folder")}: ${entry.label}`}
                  disabled={pathBusyKey !== null || props.steamCmdBusy}
                  onClick={() => void handlePathAction(entry, "open")}
                >
                  <ShellIcon name="folder" />
                </button>
                <button
                  type="button"
                  className="system-runtime-path-action-btn system-path-picker-button"
                  title={`${copy.browse}: ${entry.label}`}
                  aria-label={`${copy.browse}: ${entry.label}`}
                  disabled={pathBusyKey !== null || props.steamCmdBusy}
                  onClick={() => void handlePickPath(entry.key)}
                >
                  <ShellIcon name="folder" />
                  <span>{pathBusyKey === entry.key && pathAction === "choose" ? copy.browsing : copy.browse}</span>
                </button>
              </div>
            </div>
          ))}
        </div>
        {pathFeedback ? (
          <ActivityNotice tone={pathFeedback.error ? "error" : "success"} onDismiss={() => setPathFeedback(null)}>
            {pathFeedback.message}
          </ActivityNotice>
        ) : null}
      </div>
    </SystemPanel>
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
                const endpoint = connection ? resolveInstanceConnection({
                  summary: { ...instance, bind_ip: connection.bind_ip },
                  ports: connection.ports,
                  settings_json: connection.settings_json
                }, props.bindAddressCandidates, readPreferredJoinAddress(instance.id), locale, t) : null;
                const connectionLabel = endpoint?.endpoint ?? (instanceConnections.failed
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
                        title={endpoint ? `${endpoint.label} · ${endpoint.endpoint}` : connectionLabel}>
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

        <DefaultPathsPanel
          appSettings={props.appSettings}
          steamCmdStatus={props.steamCmdStatus}
          steamCmdBusy={props.steamCmdBusy}
          steamCmdProgress={props.steamCmdProgress}
          steamCmdMessage={props.steamCmdMessage}
          onPickDirectory={props.onPickDirectory}
          onSaveAppSettings={props.onSaveAppSettings}
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
