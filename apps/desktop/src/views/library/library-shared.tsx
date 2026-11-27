import type { TranslateFn } from "../../i18n";
import type { ModuleSummary } from "../../types";
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

export function platformLabel(platforms: string[], t: TranslateFn) {
  return platforms.length ? platforms.join(" / ") : t("common.windows");
}
