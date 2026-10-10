import type { AstroneerSaveCatalog } from "../astroneer-saves";
import type { InstanceDetails } from "../types";

export function buildMockAstroneerSaveCatalog(details: InstanceDetails): AstroneerSaveCatalog {
  if (details.summary.module_id !== "astroneer") throw new Error("Not an ASTRONEER instance.");
  const settings: unknown = JSON.parse(details.settings_json);
  if (!settings || typeof settings !== "object" || Array.isArray(settings)) throw new Error("Invalid instance settings.");
  const configuredName = "active_save_file_name" in settings ? settings.active_save_file_name : "SAVE_1";
  if (typeof configuredName !== "string") throw new Error("Invalid configured ASTRONEER save name.");
  return { instance_id: details.summary.id, configured_name: configuredName, entries: [
    { descriptive_name: "SAVE_1", latest_saved_at: "2026.10.09-10.00.00", versions: 2, total_bytes: 8388608 },
    { descriptive_name: "Custom Expedition", latest_saved_at: "2026.10.08-16.30.00", versions: 1, total_bytes: 4194304 }
  ] };
}
