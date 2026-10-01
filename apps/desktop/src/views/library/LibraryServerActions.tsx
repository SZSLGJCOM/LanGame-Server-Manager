import { useEffect, useId, useState, type ReactNode } from "react";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { ActivityNotice } from "../../components/ActivityNotice";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import type { CreateInstanceInput, InstanceProgramMode, ModuleDetails, ModuleSummary, SteamCmdStatus } from "../../types";
import { moduleRequiresSteamCmd, steamCmdInstallBlockReason } from "../../module-installation-dependency";
import { ConfigurationHelp } from "../settings/ConfigurationFieldHelp";
import { suggestedName, type LibraryInstallTone } from "./library-shared";
import { LibraryProgramInventory, useModuleProgramInventory } from "./LibraryProgramInventory";

interface LibraryServerActionsProps {
  selected: ModuleSummary;
  selectedModuleDetails: ModuleDetails | null;
  steamCmdStatus: SteamCmdStatus | null;
  steamCmdBusy: boolean;
  installLabel: string;
  installBusy: boolean;
  installStatusClass: LibraryInstallTone;
  creating: boolean;
  creationStartedAt?: number;
  instanceName: string;
  onInstall: (moduleId: string, validate: boolean) => void;
  onUninstall: (moduleId: string) => void | Promise<void>;
  onCreateServer: (input: CreateInstanceInput) => Promise<void>;
  onInstanceNameChange: (value: string) => void;
}

export function LibraryServerActions(props: LibraryServerActionsProps) {
  const { t } = useI18n();
  const instanceNameId = useId();
  const programModeId = useId();
  const [choice, setChoice] = useState<{ moduleId: string; mode: InstanceProgramMode } | null>(null);
  const supportsSharedProgram = props.selectedModuleDetails?.runtime.program_sharing === "shared";
  const programMode = supportsSharedProgram
    ? choice?.moduleId === props.selected.id ? choice.mode : "shared"
    : "independent";
  const installState = String(props.selected.install_state ?? "").toLowerCase();
  const libraryInstalled = installState === "installed";
  const programInventory = useModuleProgramInventory(props.selected, programMode, "verified", props.installBusy);
  const creationReady = programInventory.data?.creation.can_create === true && !programInventory.loading;
  const installSource = props.selectedModuleDetails?.install?.source;
  const dependencyBlock = moduleRequiresSteamCmd(props.selected, props.selectedModuleDetails?.install)
    ? steamCmdInstallBlockReason(props.steamCmdStatus, props.steamCmdBusy) : null;
  const dependencyHint = dependencyBlock ? t(`library.detail.steamCmdDependency.${dependencyBlock}`) : undefined;
  const hasManagedInstallSource =
    Boolean(props.selected.steam_app_id) ||
    Boolean(props.selectedModuleDetails?.install?.download_url_windows) ||
    installSource === "minecraft_java";
  const installActionLabel = libraryInstalled || props.installBusy
    ? props.installLabel
    : hasManagedInstallSource
      ? t("library.detail.installServerFiles")
      : t("library.detail.installSourceMissing");
  const installActionHint = !libraryInstalled && !hasManagedInstallSource ? t("library.detail.installSourceMissingHint") : undefined;
  const canInstall = !props.creating && !props.installBusy && !libraryInstalled && hasManagedInstallSource && !dependencyBlock;
  const canCheckUpdates = !props.creating && !props.installBusy && installState !== "notinstalled" && hasManagedInstallSource && !dependencyBlock;
  const hasLibraryPrograms = programInventory.data?.installations.some((installation) =>
    installation.scope === "library" && (installation.pending_removal
      || String(installation.install_state).toLowerCase() !== "notinstalled"))
    ?? installState !== "notinstalled";
  const canUninstall = !props.creating && !props.installBusy && hasLibraryPrograms && hasManagedInstallSource;
  const checkUpdatesHint = !hasManagedInstallSource ? t("library.detail.updateUnavailableNoSource") : undefined;
  const uninstallHint = !hasManagedInstallSource
    ? t("library.detail.uninstallUnavailableNoSource")
    : !hasLibraryPrograms
      ? t("library.detail.uninstallUnavailableNotInstalled")
      : undefined;
  const createHint = programInventory.loading ? t("library.programs.loading")
    : programInventory.error ? t("library.programs.readFailed", { message: programInventory.error })
      : !creationReady ? programInventory.data?.creation.reason ?? t("library.programs.creationBlocked") : t("library.detail.cleanCreationHint");

  return (
    <div className="library-sidebar-server-actions">
      <div className="library-sidebar-maintenance-row">
        <LibraryActionHint hint={!libraryInstalled ? dependencyHint ?? installActionHint : undefined} label={installActionLabel} disabled={!canInstall}>
          <button
            type="button"
            className={`primary-button library-install-state-button ${libraryInstalled ? "is-installed" : ""} ${props.installStatusClass}`.trim()}
            onClick={() => {
              if (canInstall) {
                props.onInstall(props.selected.id, false);
              }
            }}
            disabled={!canInstall}
          >
            {installActionLabel}
          </button>
        </LibraryActionHint>
        <LibraryActionHint hint={dependencyHint ?? checkUpdatesHint ?? t("library.detail.validateInstall")} label={t("library.detail.validateInstall")} disabled={!canCheckUpdates}>
          <button type="button" className="secondary-button library-maintenance-icon" aria-label={t("library.detail.validateInstall")}
            onClick={() => { if (canCheckUpdates) props.onInstall(props.selected.id, true); }} disabled={!canCheckUpdates}>
            <ShellIcon name="refresh" aria-hidden="true" />
          </button>
        </LibraryActionHint>
        <LibraryActionHint hint={uninstallHint ?? t("library.detail.uninstallServerFiles")} label={t("library.detail.uninstallServerFiles")} disabled={!canUninstall}>
          <InlineConfirmAction className="secondary-button library-maintenance-icon" disabled={!canUninstall}
            aria-label={t("library.detail.uninstallServerFiles")}
            scopeKey={props.selected.id}
            confirmation={t("library.detail.uninstallConfirm", { name: props.selected.name })}
            onConfirm={async () => {
              try { await props.onUninstall(props.selected.id); }
              finally { programInventory.refresh(); }
            }}>
            <ShellIcon name="trash" aria-hidden="true" />
          </InlineConfirmAction>
        </LibraryActionHint>
      </div>

      <form
        className="form-stack library-create-server-form library-sidebar-create-form"
        aria-busy={props.creating}
        onSubmit={(event) => {
          event.preventDefault();
          if (!creationReady || props.installBusy || props.creating) return;
          void props.onCreateServer({
            name: props.instanceName.trim() || suggestedName(props.selected.name),
            module_id: props.selected.id,
            program_mode: programMode
          }).finally(programInventory.refresh);
        }}
      >
        <div className="library-create-field">
          <label className="detail-label" htmlFor={instanceNameId}>{t("library.detail.instanceName")}</label>
          <input id={instanceNameId} className="text-input" value={props.instanceName} disabled={props.creating} onChange={(event) => props.onInstanceNameChange(event.target.value)} />
        </div>

        {supportsSharedProgram && <div className="library-program-choices">
          <ConfigurationHelp description={t("library.detail.programModeHint")}>{(help) => <div
            className="library-create-field" ref={help.anchorRef} {...help.interactionProps}>
            <label className="detail-label" htmlFor={programModeId}>{t("library.detail.programMode")}</label>
            <select id={programModeId} className="text-input" value={programMode} disabled={props.creating || props.installBusy}
              aria-describedby={help.descriptionId}
              onChange={(event) => setChoice({ moduleId: props.selected.id, mode: event.target.value === "shared" ? "shared" : "independent" })}>
              <option value="shared">{t("library.detail.sharedProgram")}</option>
              <option value="independent">{t("library.detail.independentProgram")}</option>
            </select>
          </div>}</ConfigurationHelp>
        </div>}

        <LibraryProgramInventory module={props.selected} data={programInventory.data} loading={programInventory.loading}
          error={programInventory.error} onRetry={programInventory.refresh} />
        <div className="button-row library-create-submit">
          <LibraryActionHint hint={createHint} label={t("library.detail.createServer")} disabled={!creationReady || props.installBusy || props.creating}>
            <button type="submit" className="primary-button" disabled={!creationReady || props.installBusy || props.creating}>
              {t(props.creating ? "library.detail.creatingServer" : "library.detail.createServer")}
            </button>
          </LibraryActionHint>
        </div>
        {props.creating && <CreationProgress startedAt={props.creationStartedAt} />}
      </form>
    </div>
  );
}

function LibraryActionHint({ hint, label, disabled, children }: { hint?: string; label: string; disabled: boolean; children: ReactNode }) {
  return <ConfigurationHelp description={hint}>{(help) => <span className="library-action-hint"
    ref={help.anchorRef} {...help.interactionProps} tabIndex={hint && disabled ? 0 : undefined}
    role={hint && disabled ? "group" : undefined} aria-label={hint && disabled ? label : undefined} aria-describedby={help.descriptionId}>
    {children}
  </span>}</ConfigurationHelp>;
}

function CreationProgress({ startedAt }: { startedAt?: number }) {
  const { t } = useI18n();
  const [now, setNow] = useState(Date.now);
  const [dismissed, setDismissed] = useState(false);
  useEffect(() => {
    if (dismissed) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [dismissed]);
  if (dismissed) return null;
  const seconds = Math.max(0, Math.floor((now - (startedAt ?? now)) / 1000));
  return <ActivityNotice onDismiss={() => setDismissed(true)}>{`${t("library.detail.creatingServerHint")} ${t("library.detail.creationElapsed", { seconds })}`}</ActivityNotice>;
}
