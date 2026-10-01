import { useId } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import { useI18n } from "../../i18n";
import type { InstanceArchiveSummary } from "../../storage-management-types";
import { ConfigurationWorkspace } from "../settings/ConfigurationWorkspace";
import { ModWorkbench } from "./ModWorkbench";
import { PlayerCenterWorkbench } from "./PlayerCenterWorkbench";
import { RuntimeSurfaceWorkbench } from "./RuntimeSurfaceWorkbench";
import { ServerMaintenanceWorkspace } from "./ServerMaintenanceWorkspace";
import { ServerDetailTabs, type ServerDetailTab } from "./ServerDetailTabs";
import { buildServerDetailTabSpecs } from "./server-detail-tab-specs";
import { useArchivedInstanceDetails } from "./useArchivedInstanceDetails";

export function ArchivedInstanceWorkspace({ archive, requestedTab, onTabChange }: {
  archive: InstanceArchiveSummary;
  requestedTab: ServerDetailTab;
  onTabChange: (tab: ServerDetailTab) => void;
}) {
  const { locale, t } = useI18n();
  const panelId = useId();
  const state = useArchivedInstanceDetails(archive);
  const tabs = buildServerDetailTabSpecs({ moduleId: state.details?.instance.summary.module_id ?? archive.module_id ?? "",
    moduleDetails: state.moduleDetails, t, archived: true });
  const activeTab = tabs.some((tab) => tab.id === requestedTab && !tab.disabled) ? requestedTab : "runtime";
  const retryButton = <button type="button" className="secondary-button archived-instance-workspace__retry" onClick={state.retry}>
    {t("servers.archives.workspace.retry", undefined, "Reload")}
  </button>;

  return <section className="detail-stack detail-stack--server archived-instance-workspace" data-archive-id={archive.archive_id}
    aria-label={state.details?.instance.summary.name ?? archive.instance_name ?? t("storage.unknownArchive")}
    data-archive-configuration-state={state.phase}>
    <div className="server-detail-subheader">
      <ServerDetailTabs tabs={tabs} activeTab={activeTab} panelId={panelId}
        label={t("servers.tabs.detailSectionsAria", undefined, "Instance detail sections")}
        onSelect={(tab) => { if (!tab.disabled) onTabChange(tab.id); }} />
    </div>
    <div className={`server-detail-scroll server-detail-scroll--${activeTab}`}
      id={panelId} role="tabpanel" aria-labelledby={`${panelId}-${activeTab}`} data-archive-workspace-tab={activeTab}>
      {state.phase !== "ready" ? <div className="configuration-workspace__local-state" role="status">
        {t(`servers.archives.workspace.${state.phase === "error" ? "loadFailed" : state.phase}`)}
        {state.phase === "error" ? retryButton : null}
      </div> : state.details ? <>
        {activeTab === "settings" ? <ConfigurationWorkspace key={archive.archive_id}
          details={state.details.instance} archive={state.details} moduleDetails={state.moduleDetails}
          moduleDetailsError={state.moduleError} onRetryModuleDetails={state.retry} bindAddressCandidates={[]}
          runtime={null} launchPlan={null} launchPlanError={null} /> : null}
        {activeTab === "runtime" ? <RuntimeSurfaceWorkbench details={state.details.instance} archive={state.details}
          runtime={null} runtimeWindows={null} startupPending={false} launchHostSurface="managed_terminal" /> : null}
        {activeTab === "mods" ? <ModWorkbench details={state.details.instance} moduleDetails={state.moduleDetails}
          archive={state.details} launchPlan={null} /> : null}
        {activeTab === "players" ? <PlayerCenterWorkbench details={state.details.instance} moduleDetails={state.moduleDetails}
          runtime={null} readOnly onOpenSettings={() => onTabChange("settings")} /> : null}
        <ServerMaintenanceWorkspace details={state.details.instance} moduleDetails={state.moduleDetails}
          active={activeTab === "maintenance"} archive={state.details} locale={locale} t={t} />
      </> : null}
    </div>
    {state.error ? <ActivityNotice tone="error" action={retryButton}>{state.error}</ActivityNotice> : null}
  </section>;
}
