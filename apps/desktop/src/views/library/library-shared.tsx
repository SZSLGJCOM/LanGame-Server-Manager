import type { ReactNode } from "react";
import type { TranslateFn } from "../../i18n";
import type { ModuleDetails, ModuleSummary } from "../../types";
import { formatInstallState } from "../../install-state-presentation";

export { formatInstallState } from "../../install-state-presentation";

export function hasStoredProgram(module: ModuleSummary): boolean {
  return String(module.install_state).toLowerCase() === "installed"
    || (module.instance_program_count ?? 0) > 0
    || (module.archived_program_count ?? 0) > 0;
}

export function formatProgramLocations(module: ModuleSummary, t: TranslateFn): string {
  const locations = [t("library.detail.libraryProgramState", { state: formatInstallState(module.install_state, t) })];
  if ((module.instance_program_count ?? 0) > 0) {
    locations.push(t("library.detail.instanceProgramCount", { count: module.instance_program_count! }));
  }
  if ((module.archived_program_count ?? 0) > 0) {
    locations.push(t("library.detail.archivedProgramCount", { count: module.archived_program_count! }));
  }
  return locations.join(" · ");
}

export type LibraryInstallTone = "" | "is-success" | "is-busy" | "is-danger";

export function suggestedName(moduleName: string) {
  return `${moduleName} 1`;
}

export function installTone(state: string | null | undefined): LibraryInstallTone {
  switch (String(state ?? "").toLowerCase()) {
    case "installed":
      return "is-success";
    case "installing":
    case "updating":
    case "uninstalling":
      return "is-busy";
    case "incomplete":
    case "corrupted":
    case "notinstalled":
      return "is-danger";
    default:
      return "";
  }
}

export function formatJobStatus(status: string | null | undefined, t: TranslateFn) {
  switch (String(status ?? "").toLowerCase()) {
    case "pending":
      return t("status.job.pending");
    case "running":
      return t("status.job.running");
    case "completed":
      return t("status.job.completed");
    case "failed":
      return t("status.job.failed");
    case "cancelled":
      return t("status.job.cancelled");
    default:
      return status || t("status.job.unknown");
  }
}

export function jobTone(status: string | null | undefined): LibraryInstallTone {
  switch (String(status ?? "").toLowerCase()) {
    case "pending":
    case "running":
      return "is-busy";
    case "completed":
      return "is-success";
    case "failed":
    case "cancelled":
      return "is-danger";
    default:
      return "";
  }
}

export function platformLabel(platforms: string[], t: TranslateFn) {
  return platforms.length ? platforms.join(" / ") : t("common.windows");
}

export function portsLabel(details: ModuleDetails | null, t: TranslateFn) {
  if (!details?.default_ports.length) {
    return t("library.detail.waitingModuleDetails");
  }
  return details.default_ports.map((port) => `${port.name}:${port.port}/${port.protocol}`).join("  ·  ");
}

export function technicalFact(label: string, value: string, code = false): ReactNode {
  return (
    <div className="library-fact-block" key={label}>
      <div className="detail-label">{label}</div>
      <div className={code ? "detail-value detail-value--code" : "detail-value"}>{value}</div>
    </div>
  );
}
