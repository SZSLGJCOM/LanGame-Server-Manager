import { operateArkCluster, readArkCluster } from "../../api";
import { isArkModule } from "../../ark-clusters";
import type { AiSettings } from "../../ai-settings";
import { ShellIcon } from "../../components/ShellIcon";
import type { LocaleCode, TranslateFn } from "../../i18n";
import type { InstancePanelLoadState } from "../../instance-panel-loader";
import { instanceHasRunningProcess } from "../../runtime-action-state";
import type { InstanceArchiveDetails } from "../../storage-management-types";
import type {
  BackgroundJob, DstWorldImportResult, InstanceBackupResult, InstanceDetails,
  InstanceRuntimeOverview, ModuleDetails, SaveInstanceSettingsOptions, UpdateInstanceInput
} from "../../types";
import { AiBroadcastWorkbench } from "./AiBroadcastWorkbench";
import { ArkClusterMaintenance } from "./ArkClusterMaintenance";
import { ArkClusterPanel } from "./ArkClusterPanel";
import { BackupTable } from "./BackupTable";
import { DstWorldImportPanel } from "./DstWorldImportPanel";
import { ImmediateWorldSave } from "./ImmediateWorldSave";
import { InstanceAutostartEditor } from "./InstanceAutostartEditor";
import { InstanceIsolationPanel } from "./InstanceIsolationPanel";
import { InstancePanelReadStatus } from "./InstancePanelReadStatus";
import { InstanceProgramMaintenance } from "./InstanceProgramMaintenance";
import { MoriaNativeSettingsEditor } from "./MoriaNativeSettingsEditor";
import { MaintenanceWorkspace } from "./MaintenanceWorkspace";
import { ProgramUpdatePolicyEditor } from "./ProgramUpdatePolicyEditor";
import { RuntimePerformanceEditor } from "./RuntimePerformanceEditor";
import { RuntimeRecoveryEditor } from "./RuntimeRecoveryEditor";
import { SavePolicyEditor } from "./SavePolicyEditor";
import { ScumWipeSettingsEditor } from "./ScumWipeSettingsEditor";

export interface ServerMaintenanceWorkspaceProps {
  active: boolean;
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  locale: LocaleCode;
  t: TranslateFn;
  archive?: InstanceArchiveDetails;
  selectedBackups?: InstanceBackupResult[];
  panelLoadState?: InstancePanelLoadState | null;
  jobs?: BackgroundJob[];
  runtime?: InstanceRuntimeOverview | null;
  aiSettings?: AiSettings;
  assistantCanRun?: boolean;
  onResumeAutoRefresh?: () => void;
  onCreateBackup?: (instanceId: string) => void;
  onRestoreBackup?: (instanceId: string, backupId: string) => void;
  onRenameBackup?: (instanceId: string, backup: InstanceBackupResult, displayName: string) => Promise<boolean>;
  onDeleteBackup?: (instanceId: string, backup: InstanceBackupResult) => void | Promise<void>;
  onSaveSettings?: (input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) => Promise<InstanceDetails | undefined>;
  onSaveAutostart?: (instanceId: string, autostart: boolean) => Promise<void>;
  onPickDirectory?: (currentPath?: string | null) => Promise<string | null>;
  onImportDontStarveWorldData?: (instanceId: string, sourcePath: string) => Promise<DstWorldImportResult>;
  onOpenLocalPath?: (path: string) => void;
}

function normalizePath(value: string): string {
  return String(value ?? "").replace(/\\/g, "/").replace(/\/+$/g, "").replace(/\/+/g, "/");
}
function instanceRootFromConfigFilePath(value: string): string {
  const configPath = normalizePath(value);
  const configDirectory = configPath.slice(0, configPath.lastIndexOf("/"));
  return configDirectory.slice(0, configDirectory.lastIndexOf("/"));
}

export function ServerMaintenanceWorkspace(props: ServerMaintenanceWorkspaceProps) {
  const { locale, t } = props;
  const readOnly = Boolean(props.archive);
  const onChanged = props.onResumeAutoRefresh;
  const backups = props.archive?.backups.entries ?? props.selectedBackups ?? [];
  const panelLoadState = readOnly ? undefined : props.panelLoadState;
  const canRestoreBackup = !readOnly && !instanceHasRunningProcess(props.details.summary, props.details.active_run);
  const selectedInstanceRoot = instanceRootFromConfigFilePath(props.details.config_file_path);
  const selectedBackupDirectory = selectedInstanceRoot ? normalizePath(`${selectedInstanceRoot}/backups`) : "";
  const manualBackups = backups.filter((backup) => backup.backup_kind === "manual");
  const autoStopBackups = backups.filter((backup) => backup.backup_kind === "auto_stop");
  const safeguardBackups = backups.filter((backup) => backup.backup_kind === "pre_restore");
  const backupCountsUnknown = readOnly
    ? Boolean(props.archive?.backups.truncated || props.archive?.backups.issues.length)
    : backups.length === 0 && Boolean(panelLoadState?.pending.includes("backups") || panelLoadState?.errors.backups);
  const unavailable = <p className="form-note" role="status">{t("servers.archives.workspace.restoreForMaintenance", undefined,
    "Restore this instance to use maintenance actions.")}</p>;
  return (
    <MaintenanceWorkspace active={props.active} sections={[
        { id: "backups", title: t("servers.maintenance.saves"), icon: "history",
          persistentContent: <ImmediateWorldSave details={props.details} moduleDetails={props.moduleDetails} readOnly={readOnly} />,
          content: <>
          <section aria-label={t("servers.maintenance.saves")} className="server-workbench-surface server-maintenance-card server-backup-history-card">
            <div className="server-workbench-section-label server-maintenance-card-label">
              <ShellIcon name="history" className="server-workbench-section-icon" />
              <span>{t("servers.maintenance.saves")}</span>
              <div className="server-file-backup-toolbar">
                <button type="button" className="secondary-button"
                  disabled={readOnly || !props.onCreateBackup}
                  onClick={() => { if (!readOnly) props.onCreateBackup?.(props.details.summary.id); }}>
                  {t("servers.details.backupNow")}
                </button>
              </div>
            </div>
            <div className="server-file-backup-summary">
              <div className="server-file-backup-stat"><span>{backupCountsUnknown ? "—" : manualBackups.length}</span><small>{t("servers.backups.manualTitle")}</small></div>
              <div className="server-file-backup-stat"><span>{backupCountsUnknown ? "—" : autoStopBackups.length}</span><small>{t("servers.backups.autoTitle")}</small></div>
              <div className="server-file-backup-stat"><span>{backupCountsUnknown ? "—" : safeguardBackups.length}</span><small>{t("servers.backups.safeguardTitle")}</small></div>
            </div>
            {readOnly ? unavailable : !canRestoreBackup ? <div className="server-backup-stop-note">{t("servers.backups.restoreRequiresStop")}</div> : null}
            {!props.details.backup_uses_declared_saves_path ? (
              <div className="server-backup-warning" role="note">
                <div className="detail-label">{t("servers.backups.scopeWarningTitle")}</div>
                <div className="form-note">{t("servers.backups.scopeWarningBody")}</div>
              </div>
            ) : null}
            {!readOnly && props.onResumeAutoRefresh ? <InstancePanelReadStatus state={panelLoadState} part="backups" onRetry={props.onResumeAutoRefresh} /> : null}
            {props.archive?.backups.issues.map((issue, index) => <p key={index} className="form-note form-note--error" role="status">{issue}</p>)}
            {props.archive?.backups.truncated ? <p className="form-note" role="status">{t("servers.archives.workspace.truncated")}</p> : null}
            <div className="server-backup-list-scroll" tabIndex={0} role="region" aria-label={t("servers.backups.eyebrow")}>
              {backups.length > 0 || (!backupCountsUnknown && !panelLoadState?.pending.includes("backups") && !panelLoadState?.errors.backups) ? <BackupTable
                backups={backups} locale={locale} t={t} canRestoreBackup={canRestoreBackup} readOnly={readOnly}
                instanceId={props.details.summary.id} onRestoreBackup={props.onRestoreBackup}
                onRenameBackup={props.onRenameBackup} onDeleteBackup={props.onDeleteBackup}
              /> : null}
            </div>
          </section>
          {!readOnly && props.details.summary.module_id === "dontstarve" && props.onPickDirectory && props.onImportDontStarveWorldData ? (
            <DstWorldImportPanel key={`import-${props.details.summary.id}`} details={props.details}
              onPickDirectory={props.onPickDirectory} onImportWorldData={props.onImportDontStarveWorldData} />
          ) : null}
        </> },
        { id: "save-policy", title: t("servers.maintenance.savePolicy"), icon: "database", content: <>
          <section aria-label={t("servers.maintenance.savePolicy")} className="server-workbench-surface server-maintenance-card server-backup-policy-card">
            <div className="server-workbench-section-label server-maintenance-card-label">
              <ShellIcon name="database" className="server-workbench-section-icon" />
              <span>{t("servers.maintenance.savePolicy")}</span>
            </div>
            <SavePolicyEditor details={props.details} moduleDetails={props.moduleDetails}
              locale={locale} t={t} readOnly={readOnly} onSaveSettings={props.onSaveSettings} />
          </section>
        </> },
        { id: "runtime", title: t("servers.maintenance.runtimePolicy"), icon: "settings", content: <>
          <RuntimePerformanceEditor details={props.details} t={t}
            readOnly={readOnly} appliedLimits={readOnly ? undefined : props.runtime?.performance.applied_resource_limits}
            onSaveSettings={props.onSaveSettings} />
          <section className="server-workbench-surface server-maintenance-card server-runtime-policy-card"
            aria-label={t("servers.maintenance.runtimePolicy")}>
            <div className="server-workbench-section-label server-maintenance-card-label">
              <ShellIcon name="settings" className="server-workbench-section-icon" />
              <span>{t("servers.maintenance.runtimePolicy")}</span>
            </div>
            <InstanceAutostartEditor details={props.details} t={t} readOnly={readOnly} jobs={readOnly ? undefined : props.jobs}
              onSaveAutostart={props.onSaveAutostart} />
            <RuntimeRecoveryEditor details={props.details} t={t} readOnly={readOnly}
              savedCrashRestartLimit={props.archive?.maintenance.crash_restart_limit} onSaveSettings={props.onSaveSettings} />
          </section>
        </> },
        { id: "storage", title: t("servers.isolation.title"), icon: "folder", content: <>
          <ProgramUpdatePolicyEditor details={props.details} moduleDetails={props.moduleDetails} t={t}
            readOnly={readOnly} onSaveSettings={props.onSaveSettings} />
          {readOnly ? unavailable : props.onOpenLocalPath && onChanged ? <InstanceIsolationPanel key={`isolation-${props.details.summary.id}`}
            instanceId={props.details.summary.id} backupPath={selectedBackupDirectory}
            renderProgramMaintenance={(report) => <InstanceProgramMaintenance
              details={props.details!} report={report} jobs={props.jobs ?? []}
              onChanged={onChanged} />}
            onOpenLocalPath={props.onOpenLocalPath} /> : null}
        </> },
        ...(props.details.summary.module_id === "scum" ? [{
          id: "scum-wipe", title: t("scum.maintenance.title"), icon: "database" as const,
          content: <ScumWipeSettingsEditor details={props.details} moduleDetails={props.moduleDetails}
            locale={locale} t={t} readOnly={readOnly} onSaveSettings={props.onSaveSettings} />
        }] : []),
        ...(props.details.summary.module_id === "returntomoria" ? [{
          id: "world-upgrade", title: t("returntomoria.workspace.worldUpgradeTitle"), icon: "database" as const,
          content: <MoriaNativeSettingsEditor details={props.details} moduleDetails={props.moduleDetails}
            kind="world-upgrade" locale={locale} t={t} readOnly={readOnly} onSaveSettings={props.onSaveSettings} />
        }] : []),
        ...(isArkModule(props.details.summary.module_id) ? [{
          id: "cluster", title: locale === "zh-CN" ? "ARK 集群" : "ARK cluster", icon: "server" as const,
          content: <>
            {readOnly ? unavailable : props.onResumeAutoRefresh ? <ArkClusterPanel instanceId={props.details.summary.id}
              readReport={readArkCluster} operate={operateArkCluster}
              onChanged={props.onResumeAutoRefresh}
              renderMaintenance={(report, refresh, busy, onBusyChange) =>
                <ArkClusterMaintenance instanceId={props.details!.summary.id} report={report}
                  busy={busy} onChanged={refresh} onBusyChange={onBusyChange} />} /> : null}
          </>
        }] : [])
      ]} broadcast={
        readOnly ? unavailable : props.aiSettings ? <AiBroadcastWorkbench
          active={props.active}
          aiSettings={props.aiSettings}
          assistantCanRun={Boolean(props.assistantCanRun)}
          details={props.details}
          moduleDetails={props.moduleDetails}
          runtime={props.runtime ?? null}
        /> : null
      } />
  );
}
