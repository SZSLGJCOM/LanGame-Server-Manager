import { useEffect, useRef, useState } from "react";
import { inspectModulePrograms } from "../../api-storage";
import { describeError } from "../../app-state";
import { formatDownloadBytes } from "../../download-rate";
import { useI18n } from "../../i18n";
import { ShellIcon, type ShellIconName } from "../../components/ShellIcon";
import { ConfigurationHelp } from "../settings/ConfigurationFieldHelp";
import type { ModuleProgramInventory } from "../../storage-management-types";
import type { InstanceProgramMode, ModuleSummary } from "../../types";
import { formatProgramLocations } from "./library-shared";
import "./library-program-inventory.css";

export function useModuleProgramInventory(module: ModuleSummary, mode: InstanceProgramMode, source: "verified" | "local", busy: boolean) {
  const [revision, setRevision] = useState(0);
  const [result, setResult] = useState<{ key: string; data: ModuleProgramInventory | null; error: string | null } | null>(null);
  const reader = useRef<{ active: boolean; desired: {
    key: string; moduleId: string; mode: InstanceProgramMode; source: "verified" | "local";
  } | null }>({ active: false, desired: null });
  const key = JSON.stringify([module.id, mode, source, module.install_state, module.instance_program_count,
    module.archived_program_count, busy, revision]);
  useEffect(() => {
    const desired = busy ? null : { key, moduleId: module.id, mode, source };
    reader.current.desired = desired;
    async function readLatest() {
      if (reader.current.active) return;
      reader.current.active = true;
      try {
        // A native scan has its own deadline. Keep at most one in flight and
        // replace queued selections instead of starting a scan for every click.
        while (reader.current.desired) {
          const request = reader.current.desired;
          try {
            const data = await inspectModulePrograms(request.moduleId, request.mode, request.source);
            if (reader.current.desired === request) setResult({ key: request.key, data, error: null });
          } catch (error) {
            if (reader.current.desired === request) setResult({ key: request.key, data: null, error: describeError(error) });
          }
          if (reader.current.desired === request) break;
        }
      } finally { reader.current.active = false; }
    }
    if (desired) void readLatest();
    return () => { if (reader.current.desired === desired) reader.current.desired = null; };
  }, [key]);
  return {
    data: result?.key === key ? result.data : null,
    error: result?.key === key ? result.error : null,
    loading: busy || result?.key !== key,
    refresh: () => setRevision((value) => value + 1)
  };
}

export function LibraryProgramInventory({ module, data, loading, error, onRetry }: {
  module: ModuleSummary; data: ModuleProgramInventory | null; loading: boolean; error: string | null; onRetry: () => void;
}) {
  const { t, locale } = useI18n();
  const bytes = (value: number | null) => value === null ? t("library.programs.sizeUnknown") : formatDownloadBytes(value, locale);
  const installedCount = data?.installations.filter((installation) =>
    String(installation.install_state).toLowerCase() === "installed").length ?? 0;
  const installationHelp = [
    String(module.install_state).toLowerCase() !== "installed" ? formatProgramLocations(module, t) : null,
    ...(data?.installations.map((installation) => [
      installation.install_root,
      `${bytes(installation.size_bytes)} · ${t(`library.programs.modification.${installation.modification_state}`)}`,
      installation.used_by.length ? t("library.programs.usedBy", { names: installation.used_by.map((instance) => instance.name).join(locale.startsWith("zh") ? "、" : ", ") })
        : null,
      installation.current_version ? t("library.programs.version", { version: installation.current_version }) : null
    ].filter(Boolean).join("\n")) ?? []),
    !data?.installations.length ? data?.creation.program_path : null
  ].filter(Boolean).join("\n\n");
  const creationHelp = data ? [
    data.creation.can_create ? t(`library.programs.action.${data.creation.action}`)
      : data.creation.reason ?? t("library.programs.creationBlocked"),
    data.creation.can_create ? t("library.programs.additionalSpace", { size: bytes(data.creation.additional_bytes) }) : null
  ].filter(Boolean).join("\n") : "";
  const archiveCount = module.archived_program_count ?? 0;
  return <div className="library-program-inventory" aria-busy={loading}>
    {loading ? <span role="status">{t("library.programs.loading")}</span> : error ? <>
      <ProgramMetric icon="alert-circle" label={t("library.programs.readFailedShort")}
        help={t("library.programs.readFailed", { message: error })} status="alert" />
      <button type="button" className="secondary-button" onClick={onRetry}>{t("common.retry")}</button>
    </> : data ? <>
      <ProgramMetric icon="package" label={t("library.programs.countShort", { count: installedCount })}
        accessibleLabel={t("library.programs.count", { count: installedCount })} help={installationHelp} />
      <ProgramMetric icon={data.creation.can_create ? "hard-drive" : "alert-circle"}
        label={data.creation.can_create ? data.creation.additional_bytes === null ? t("library.programs.sizeUnknown")
          : `+${bytes(data.creation.additional_bytes)}` : t("library.programs.creationBlocked")}
        accessibleLabel={data.creation.can_create ? t("library.programs.additionalSpace", { size: bytes(data.creation.additional_bytes) })
          : t("library.programs.creationBlocked")}
        help={creationHelp} status={data.creation.can_create ? undefined : "status"} />
    </> : null}
    {archiveCount > 0 && <ProgramMetric icon="history" label={t("library.programs.archivesShort", { count: archiveCount })}
      help={`${t("library.detail.archivedProgramCount", { count: archiveCount })}\n${t("library.detail.archivedProgramHint")}`} />}
  </div>;
}

function ProgramMetric({ icon, label, accessibleLabel, help, status }: {
  icon: ShellIconName; label: string; accessibleLabel?: string; help: string; status?: "alert" | "status";
}) {
  return <ConfigurationHelp description={help}>{(binding) =>
    <span className="library-program-metric" ref={binding.anchorRef} {...binding.interactionProps}
      tabIndex={0} role={status ?? "note"} aria-label={accessibleLabel ?? label} aria-describedby={binding.descriptionId}>
      <ShellIcon name={icon} aria-hidden="true" /><span>{label}</span>
    </span>
  }</ConfigurationHelp>;
}
