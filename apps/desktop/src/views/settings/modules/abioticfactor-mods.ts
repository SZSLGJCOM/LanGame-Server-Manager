import type { TranslateFn } from "../../../i18n";
import type { SettingsObject } from "../settings-schema";

const DOWNLOAD_DIRECTORY = "AbioticFactor/Saved/mod.io";
const SIDELOAD_DIRECTORY = "AbioticFactor/Mods";

function validDirectory(value: string, download: boolean): boolean {
  const parts = value.split(/[\\/]/);
  return parts.length >= (download ? 3 : 2)
    && parts[0].toLowerCase() === "abioticfactor"
    && parts[1].toLowerCase() === (download ? "saved" : "mods")
    && parts.every((part) => part.length > 0 && part !== "." && part !== ".."
      && !/[. ]$/.test(part) && !/[\x00-\x1f\x7f<>:"|?*]/.test(part)
      && !/^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(part));
}

export function validateAbioticModField(
  key: string,
  value: unknown,
  settings: SettingsObject,
  t: TranslateFn
): string | undefined {
  const ids = String(settings.mod_ids ?? "").trim().split(/[\s,;]+/).filter(Boolean);
  if (key === "mod_ids") {
    if (ids.some((id) => !/^[1-9]\d{0,19}$/.test(id) || BigInt(id) > 18446744073709551615n)) {
      return t("abiotic.settings.validation.modIds", undefined,
        "Use positive mod.io numeric IDs separated by newlines, commas or semicolons.");
    }
    if (settings.sideload_mods === true && ids.length === 0) {
      return t("abiotic.settings.validation.sideloadIds", undefined,
        "Sideloaded Mods still require their mod.io IDs in the official Mod list.");
    }
  }
  if (key === "modio_token" && /[\s\x00-\x1f\x7f"]/.test(String(value ?? "").trim())) {
    return t("abiotic.settings.validation.modioToken", undefined,
      "The mod.io token cannot contain whitespace, control characters or double quotes.");
  }
  if (key === "mods_directory" || key === "sideload_mods_path") {
    const download = key === "mods_directory";
    const text = typeof value === "string" ? value.trim() : download ? DOWNLOAD_DIRECTORY : SIDELOAD_DIRECTORY;
    if (!validDirectory(text, download)) {
      return download
        ? t("abiotic.settings.validation.downloadDirectory", undefined,
          "Choose a relative subdirectory inside this instance's AbioticFactor/Saved directory. Absolute paths, parent traversal and Windows special names are not allowed.")
        : t("abiotic.settings.validation.sideloadDirectory", undefined,
          "Choose AbioticFactor/Mods or one of its subdirectories in this instance. Absolute paths, parent traversal and Windows special names are not allowed.");
    }
    if (download && ids.length > 0 && settings.sideload_mods !== true
      && !String(settings.modio_token ?? "").trim() && text !== DOWNLOAD_DIRECTORY) {
      return t("abiotic.settings.validation.directoryToken", undefined,
        "A custom download directory requires a mod.io token. Without a token, the server uses its default directory and downloads Mods again at each startup.");
    }
  }
  return undefined;
}
