import { useRef, useState } from "react";
import { openLocalPath } from "../../api";
import { ActivityNotice } from "../../components/ActivityNotice";
import { ShellIcon } from "../../components/ShellIcon";
import { isChineseLocale, useI18n } from "../../i18n";
import type { StorageMigrationModel } from "../../hooks/useStorageMigration";
import { storageMigrationActive } from "../../storage-migration-types";
import type { AppPathSettingsInput, AppSettings, SteamCmdPrepareSnapshot, SteamCmdStatus } from "../../types";
import { appPathSettingsEqual, createAppPathSelection, createAppPathSettingsDraft, isStorageVolumeRoot } from "./app-path-settings-draft";

interface StoragePathsPanelProps {
  appSettings: AppSettings;
  steamCmdStatus: SteamCmdStatus | null;
  steamCmdBusy: boolean;
  steamCmdProgress: SteamCmdPrepareSnapshot | null;
  steamCmdMessage: string;
  onPickDirectory: (currentPath?: string | null) => Promise<string | null>;
  storageMigration: StorageMigrationModel;
  onEnsureSteamCmd: () => void;
  onUninstallSteamCmd: () => void;
}

export function StoragePathsPanel(props: StoragePathsPanelProps) {
  const { locale, t } = useI18n();
  const [pathBusyKey, setPathBusyKey] = useState<keyof AppPathSettingsInput | null>(null);
  const [pathAction, setPathAction] = useState<"choose" | "open" | "copy" | null>(null);
  const [pathFeedback, setPathFeedback] = useState<{ message: string; tone: "info" | "success" | "error" } | null>(null);
  const migration = props.storageMigration;
  const status = migration.status;
  const configuredPaths = status?.phase === "completed" && (migration.refreshing || migration.refreshError)
    ? status.input : createAppPathSettingsDraft(props.appSettings);
  const migrationBusy = migration.recovering || migration.pending !== null || migration.unconfirmed
    || migration.cancelRequested || migration.refreshing || Boolean(status?.recovery_required) || storageMigrationActive(status);
  const pathOperationBusy = pathBusyKey !== null || migrationBusy || props.steamCmdBusy;
  const latest = useRef({ configuredPaths, migration, migrationBusy, steamCmdBusy: props.steamCmdBusy });
  latest.current = { configuredPaths, migration, migrationBusy, steamCmdBusy: props.steamCmdBusy };
  const executablePath = props.steamCmdStatus?.executable_path || t("system.runtimeUnknownPath");
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
        rootSelected: "已自动选择 LanGame 子目录",
        failed: "路径操作失败",
        unavailable: "请在运行 LGSM 的本机 Windows 桌面端迁移目录；浏览器预览不读取或移动本机文件。"
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
        rootSelected: "Automatically selected a LanGame subfolder",
        failed: "Path action failed",
        unavailable: "Migrate folders from the local Windows LGSM desktop host. Browser previews do not read or move local files."
      };
  const pathEntries = [
    {
      key: "games_root" as const,
      icon: "package" as const,
      label: copy.gamesRoot,
      value: configuredPaths.games_root
    },
    {
      key: "servers_root" as const,
      icon: "database" as const,
      label: copy.serversRoot,
      value: configuredPaths.servers_root
    },
    {
      key: "archives_root" as const,
      icon: "inbox" as const,
      label: copy.archivesRoot,
      value: configuredPaths.archives_root
    },
    {
      key: "steamcmd_root" as const,
      icon: "terminal" as const,
      label: copy.steamCmdRoot,
      value: configuredPaths.steamcmd_root
    }
  ];

  const showPathError = (error: unknown) => {
    const detail = error instanceof Error ? error.message : String(error);
    setPathFeedback({ message: `${copy.failed}: ${detail}`, tone: "error" });
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
      setPathFeedback({ message: `${action === "open" ? copy.opened : copy.copied}: ${entry.label}`, tone: "success" });
    } catch (error) {
      showPathError(error);
    } finally {
      setPathBusyKey(null);
      setPathAction(null);
    }
  }

  async function handlePickPath(key: Exclude<keyof AppPathSettingsInput, "archives_root">) {
    if (pathOperationBusy || !migration.available) return;
    setPathBusyKey(key);
    setPathAction("choose");
    setPathFeedback(null);
    try {
      const selected = await props.onPickDirectory(configuredPaths[key]);
      const nextValue = selected?.trim();
      const live = latest.current;
      if (!nextValue || nextValue === live.configuredPaths[key] || live.migrationBusy || live.steamCmdBusy) return;
      const next = createAppPathSelection(live.configuredPaths, key, nextValue);
      if (isStorageVolumeRoot(nextValue)) {
        setPathFeedback({ message: `${copy.rootSelected}: ${next[key]}`, tone: "info" });
      }
      if (!appPathSettingsEqual(next, live.configuredPaths)) await live.migration.migrate(next);
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
    <section className="system-hud-panel system-runtime-panel">
      <div className="system-panel-head">
        <div><h2>{copy.title}</h2><span>{copy.eyebrow}</span></div>
        <div className="system-panel-aside">
          <div className="system-runtime-header-actions">
            {props.steamCmdStatus?.can_uninstall && (
              <button
                type="button"
                className="danger"
                onClick={props.onUninstallSteamCmd}
                disabled={pathOperationBusy}
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
              disabled={pathOperationBusy}
            >
              <span className="status-dot" aria-hidden="true" />
              <span>{props.steamCmdBusy
                ? props.steamCmdProgress ? t(`steamcmd.prepare.${props.steamCmdProgress.phase}`) : t("system.runtimeChecking")
                : props.steamCmdProgress?.error ? t("steamcmd.retry") : t("system.runtimeCheckSteamCmd")}</span>
            </button>
          </div>
        </div>
      </div>
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
                  disabled={pathOperationBusy}
                  onClick={() => void handlePathAction(entry, "copy")}
                >
                  <ShellIcon name="copy" />
                </button>
                <button
                  type="button"
                  className="system-runtime-path-action-btn"
                  title={t("system.runtimeOpenFolder", undefined, "Open folder")}
                  aria-label={`${t("system.runtimeOpenFolder", undefined, "Open folder")}: ${entry.label}`}
                  disabled={pathOperationBusy}
                  onClick={() => void handlePathAction(entry, "open")}
                >
                  <ShellIcon name="folder" />
                </button>
                {entry.key !== "archives_root" ? <button
                  type="button"
                  className="system-runtime-path-action-btn system-path-picker-button"
                  title={migration.available ? `${copy.browse}: ${entry.label}` : copy.unavailable}
                  aria-label={`${copy.browse}: ${entry.label}`}
                  disabled={pathOperationBusy || !migration.available}
                  onClick={() => void handlePickPath(entry.key)}
                >
                  <ShellIcon name="folder" />
                  <span>{pathBusyKey === entry.key && pathAction === "choose" ? copy.browsing : copy.browse}</span>
                </button> : null}
              </div>
            </div>
          ))}
        </div>
        {pathFeedback ? (
          <ActivityNotice tone={pathFeedback.tone} onDismiss={() => setPathFeedback(null)}>
            {pathFeedback.message}
          </ActivityNotice>
        ) : null}
      </div>
    </section>
  );
}
