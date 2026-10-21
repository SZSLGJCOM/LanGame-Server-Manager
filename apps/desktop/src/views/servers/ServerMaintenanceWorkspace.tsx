import { operateArkCluster, readArkCluster } from "../../api";
import { isArkModule } from "../../ark-clusters";
import type { AiSettings } from "../../ai-settings";
import { ShellIcon } from "../../components/ShellIcon";
import { ConfigurationHelp } from "../settings/ConfigurationFieldHelp";
import type { LocaleCode, TranslateFn } from "../../i18n";
import type { InstancePanelLoadState } from "../../instance-panel-loader";
import { instanceHasRunningProcess } from "../../runtime-action-state";
import type { InstanceArchiveDetails } from "../../storage-management-types";
import type { SteamInstallationAccountContext } from "../../steam-installation-account";
import type {
  BackgroundJob, DstWorldImportResult, InstanceBackupResult, InstanceDetails,
  InstanceRuntimeOverview, ModuleDetails, SaveInstanceSettingsOptions, UpdateInstanceInput
} from "../../types";
import { AiBroadcastWorkbench } from "./AiBroadcastWorkbench";
import { ArkClusterPanel } from "./ArkClusterPanel";
import { ArkClusterSnapshotSection } from "./ArkClusterSnapshotSection";
import { BackupTable } from "./BackupTable";
import { ImmediateWorldSave } from "./ImmediateWorldSave";
import { DailyMaintenanceEditor } from "./DailyMaintenanceEditor";
import { nativeSavePolicyKeys } from "../settings/save-policy";
import { InstanceAutostartEditor } from "./InstanceAutostartEditor";
import { InstanceIsolationPanel } from "./InstanceIsolationPanel";
import { InstancePanelReadStatus } from "./InstancePanelReadStatus";
import { InstanceProgramMaintenance } from "./InstanceProgramMaintenance";
import { MoriaNativeSettingsEditor } from "./MoriaNativeSettingsEditor";
import { MaintenanceWorkspace } from "./MaintenanceWorkspace";
import { MaintenanceAction, MaintenanceChip, MaintenanceGroup, MaintenanceHelp, MaintenanceToolbar } from "./MaintenanceToolbar";
import { ProgramUpdatePolicyEditor } from "./ProgramUpdatePolicyEditor";
import { RuntimePerformanceEditor } from "./RuntimePerformanceEditor";
import { RuntimeRecoveryEditor } from "./RuntimeRecoveryEditor";
import { SavePolicyEditor } from "./SavePolicyEditor";
import { SaveSlotTable } from "./SaveSlotTable";
import { SaveImportPanel } from "./SaveImportPanel";
import { ScumWipeSettingsEditor } from "./ScumWipeSettingsEditor";

export interface ServerMaintenanceWorkspaceProps {
  active: boolean;
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  locale: LocaleCode;
  t: TranslateFn;
  archive?: InstanceArchiveDetails;
  account?: SteamInstallationAccountContext;
  accountBusy?: boolean;
  keepProgramMaintenance?: boolean;
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

export function ServerMaintenanceWorkspace(props: ServerMaintenanceWorkspaceProps) {
  const { locale, t } = props;
  const readOnly = Boolean(props.archive);
  const moduleId = props.details.summary.module_id;
  // No native save setting in the capability table: the chip explains how the game saves instead of opening an empty editor.
  const autoSaveUnavailable = nativeSavePolicyKeys(moduleId).length === 0 ? [
    t(`servers.savePolicy.mechanism.${moduleId}`, undefined, t("servers.savePolicy.gameManaged")),
    `${t("servers.savePolicy.frequency")}${locale === "zh-CN" ? "：" : ": "}${t(`servers.savePolicy.frequency.${moduleId}`, undefined, t("servers.savePolicy.frequency.gameManaged"))}`
  ].join("\n") : null;
  const onChanged = props.onResumeAutoRefresh;
  const backups = props.archive?.backups.entries ?? props.selectedBackups ?? [];
  const panelLoadState = readOnly ? undefined : props.panelLoadState;
  const canRestoreBackup = !readOnly && !instanceHasRunningProcess(props.details.summary, props.details.active_run);
  const manualBackups = backups.filter((backup) => backup.backup_kind === "manual");
  const autoStopBackups = backups.filter((backup) => backup.backup_kind === "auto_stop");
  const safeguardBackups = backups.filter((backup) => backup.backup_kind === "pre_restore");
  const backupCountsUnknown = readOnly
    ? Boolean(props.archive?.backups.truncated || props.archive?.backups.issues.length)
    : backups.length === 0 && Boolean(panelLoadState?.pending.includes("backups") || panelLoadState?.errors.backups);
  const status = props.details.summary.status.toLowerCase();
  const statusTag = <span className={`runtime-status-tag is-${status}`}>{t(`status.instance.${status}`, undefined, props.details.summary.status)}</span>;
  // Runtime policies: a balanced grid of settings chips; each popover holds only its controls.
  const policies = <MaintenanceGroup className="runtime-policies" label={t("servers.maintenance.policiesGroup")}
    help={t("servers.maintenance.policiesHelp")}>
    <div className="runtime-policies__grid">
      <RuntimePerformanceEditor details={props.details} t={t}
        readOnly={readOnly} appliedLimits={readOnly ? undefined : props.runtime?.performance.applied_resource_limits}
        onSaveSettings={props.onSaveSettings} />
      <InstanceAutostartEditor details={props.details} t={t} readOnly={readOnly} jobs={readOnly ? undefined : props.jobs}
        onSaveAutostart={props.onSaveAutostart} />
      <RuntimeRecoveryEditor details={props.details} t={t} readOnly={readOnly}
        savedCrashRestartLimit={props.archive?.maintenance.crash_restart_limit} onSaveSettings={props.onSaveSettings} />
      <DailyMaintenanceEditor instanceId={props.details.summary.id} action="restart" chip readOnly={readOnly} t={t} />
      <ProgramUpdatePolicyEditor details={props.details} moduleDetails={props.moduleDetails} t={t}
        readOnly={readOnly} onSaveSettings={props.onSaveSettings} />
    </div>
  </MaintenanceGroup>;
  const unavailable = <p className="form-note" role="status">{t("servers.archives.workspace.restoreForMaintenance", undefined,
    "Restore this instance to use maintenance actions.")}</p>;
  return (
    <MaintenanceWorkspace active={props.active} sections={[
        { id: "saves", title: t("servers.maintenance.savesTitle"), icon: "folder", preserveAfterVisit: true,
          persistentContent: <ImmediateWorldSave details={props.details} moduleDetails={props.moduleDetails} readOnly={readOnly}>
            <MaintenanceChip icon="clock" label={t("servers.maintenance.autoSave")} className="server-save-policy-card"
              disabledReason={autoSaveUnavailable} help={t(moduleId === "unturned" ? "servers.savePolicy.managedEffective" : "servers.savePolicy.nativeEffective")}>
              <SavePolicyEditor scope="native" details={props.details} moduleDetails={props.moduleDetails}
                locale={locale} t={t} readOnly={readOnly} onSaveSettings={props.onSaveSettings} />
            </MaintenanceChip>
            <SaveImportPanel key={`import-${props.details.summary.id}`} details={props.details} readOnly={readOnly}
              onPickDirectory={props.onPickDirectory} onImportDontStarveWorldData={props.onImportDontStarveWorldData}
              onChanged={onChanged} />
          </ImmediateWorldSave>,
          content: <>
          <section aria-label={t("servers.saveSlots.title")} className="server-workbench-surface server-maintenance-card server-save-files-card">
            {readOnly ? unavailable : <SaveSlotTable instanceId={props.details.summary.id} locale={locale} t={t}
              onOpenLocalPath={props.onOpenLocalPath} />}
          </section>
        </> },
        { id: "backups", title: t("servers.maintenance.backupsTitle"), icon: "history",
          content: <>
          <section aria-label={t("servers.maintenance.backupHistory")} className="server-workbench-surface server-maintenance-card server-backup-history-card">
            <div className="server-workbench-section-label server-maintenance-card-label">
              <ShellIcon name="history" className="server-workbench-section-icon" />
              <ConfigurationHelp description={backupCountsUnknown ? null : [
                `${t("servers.backups.manualTitle")} ${manualBackups.length}`,
                `${t("servers.backups.autoTitle")} ${autoStopBackups.length}`,
                `${t("servers.backups.safeguardTitle")} ${safeguardBackups.length}`].join(" · ")}>{(help) =>
                <span ref={help.anchorRef} {...help.interactionProps} tabIndex={help.descriptionId ? 0 : undefined}
                  aria-describedby={help.descriptionId}>{t("servers.maintenance.backupHistory")}
                  <span className="server-backup-count">{backupCountsUnknown ? "—" : backups.length}</span></span>}</ConfigurationHelp>
              <MaintenanceToolbar label={t("servers.maintenance.backupHistory")}>
                <MaintenanceAction icon="plus" className="secondary-button" label={t("servers.details.backupNow")}
                  disabledReason={readOnly ? t("servers.archives.workspace.restoreForMaintenance") : null}
                  disabled={!props.onCreateBackup}
                  onClick={() => { if (!readOnly) props.onCreateBackup?.(props.details.summary.id); }} />
                <MaintenanceChip icon="clock" label={t("servers.maintenance.backupPolicy")} className="server-backup-policy-card">
                  <SavePolicyEditor scope="backup" details={props.details} moduleDetails={props.moduleDetails}
                    locale={locale} t={t} readOnly={readOnly} onSaveSettings={props.onSaveSettings} />
                  <DailyMaintenanceEditor instanceId={props.details.summary.id} action="backup" readOnly={readOnly} t={t} />
                </MaintenanceChip>
              </MaintenanceToolbar>
            </div>
            {readOnly ? unavailable : null}
            {!props.details.backup_uses_declared_saves_path ? (
              <ConfigurationHelp description={t("servers.backups.scopeWarningBody")}>{(help) =>
                <p className="form-note server-backup-warning" role="note" ref={help.anchorRef} {...help.interactionProps} tabIndex={0} aria-describedby={help.descriptionId}>
                  <ShellIcon name="alert-triangle" className="maintenance-help-icon" />{t("servers.backups.scopeWarningTitle")}</p>}</ConfigurationHelp>
            ) : null}
            {!readOnly && props.onResumeAutoRefresh ? <InstancePanelReadStatus state={panelLoadState} part="backups" onRetry={props.onResumeAutoRefresh} /> : null}
            {props.archive?.backups.issues.map((issue, index) => <p key={index} className="form-note form-note--error" role="status">{issue}</p>)}
            {props.archive?.backups.truncated ? <p className="form-note" role="status">{t("servers.archives.workspace.truncated")}</p> : null}
            <div className="server-backup-list-scroll" tabIndex={0} role="region" aria-label={t("servers.backups.eyebrow")}>
              {backups.length > 0 || (!backupCountsUnknown && !panelLoadState?.pending.includes("backups") && !panelLoadState?.errors.backups) ? <BackupTable
                backups={backups} locale={locale} t={t} canRestoreBackup={canRestoreBackup} readOnly={readOnly}
                instanceId={props.details.summary.id} onRestoreBackup={props.onRestoreBackup}
                onRenameBackup={props.onRenameBackup} onDeleteBackup={props.onDeleteBackup}
                onOpenLocalPath={props.onOpenLocalPath}
              /> : null}
            </div>
          </section>
          {isArkModule(props.details.summary.module_id) ? <section aria-label={t("servers.maintenance.clusterBackupTitle")}
            className="server-workbench-surface server-maintenance-card ark-cluster-backup-section">
            <div className="server-workbench-section-label server-maintenance-card-label">
              <ShellIcon name="server" className="server-workbench-section-icon" />
              <MaintenanceHelp help={t("servers.maintenance.clusterBackupBody")}>{t("servers.maintenance.clusterBackupTitle")}</MaintenanceHelp>
            </div>
            {readOnly ? unavailable : <ArkClusterSnapshotSection instanceId={props.details.summary.id} onChanged={onChanged} />}
          </section> : null}
        </> },
        { id: "runtime", title: t("servers.maintenance.run"), icon: "server", preserveAfterVisit: props.keepProgramMaintenance, content: <>
          {readOnly || !props.onOpenLocalPath || !onChanged ? <section aria-label={t("servers.maintenance.run")}
            className="server-workbench-surface server-maintenance-card instance-isolation-panel">
            <div className="server-workbench-section-label server-maintenance-card-label">
              <ShellIcon name="server" className="server-workbench-section-icon" />
              <span>{t("servers.maintenance.run")}</span>
              {statusTag}
            </div>
            {readOnly ? unavailable : null}
            {policies}
          </section> : <InstanceIsolationPanel key={`isolation-${props.details.summary.id}`}
            instanceId={props.details.summary.id} title={t("servers.maintenance.run")} status={statusTag} policies={policies}
            renderProgramMaintenance={(report) => <InstanceProgramMaintenance
              details={props.details!} report={report} jobs={props.jobs ?? []}
              account={props.account} accountBusy={props.accountBusy}
              onChanged={onChanged} />}
            onOpenLocalPath={props.onOpenLocalPath} />}
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
          id: "cluster", title: t("servers.maintenance.cluster"), icon: "server" as const,
          content: <>
            {readOnly ? unavailable : props.onResumeAutoRefresh ? <ArkClusterPanel instanceId={props.details.summary.id}
              readReport={readArkCluster} operate={operateArkCluster}
              onChanged={props.onResumeAutoRefresh} /> : null}
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
