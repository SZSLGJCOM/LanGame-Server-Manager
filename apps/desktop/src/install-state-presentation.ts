import type { TranslateFn } from "./i18n";

export function formatInstallState(state: string | null | undefined, t: TranslateFn) {
  switch (String(state ?? "").toLowerCase()) {
    case "installed":
      return t("status.install.installed");
    case "installing":
      return t("status.install.installing");
    case "incomplete":
      return t("status.install.incomplete");
    case "updating":
      return t("status.install.updating");
    case "uninstalling":
      return t("status.install.uninstalling");
    case "corrupted":
      return t("status.install.corrupted");
    case "notinstalled":
      return t("status.install.notinstalled");
    default:
      return state || t("status.install.unknown");
  }
}
