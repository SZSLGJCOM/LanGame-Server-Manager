import type { SettingsModuleDefinition } from "./module-types";

export interface NativeSavePolicy {
  autosave: readonly string[];
  backups: readonly string[];
}

// Exact native settings, not a name heuristic: log rotation, save selection,
// restore behavior and player-integrity settings keep their existing owners.
export const NATIVE_SAVE_POLICIES: Readonly<Record<string, NativeSavePolicy>> = {
  arksurvivalascended: { autosave: ["auto_save_period_minutes"], backups: [] },
  arksurvivalevolved: { autosave: ["auto_save_period_minutes"], backups: ["max_num_of_save_backups"] },
  astroneer: { autosave: ["auto_save_interval_seconds"], backups: ["backup_save_interval_seconds"] },
  dontstarve: { autosave: ["autosaver_enabled"], backups: ["max_snapshots"] },
  humanitz: { autosave: ["save_interval_seconds"], backups: [] },
  palworld: { autosave: ["auto_save_span"], backups: ["use_backup_save_data"] },
  projectzomboid: {
    autosave: ["save_world_every_minutes"],
    backups: ["backups_on_start", "backups_on_version_change", "backups_period", "backups_count"]
  },
  rust: { autosave: ["save_interval_seconds"], backups: [] },
  satisfactory: { autosave: [], backups: ["rotating_autosaves"] },
  sonsoftheforest: { autosave: ["save_interval"], backups: [] },
  soulmask: { autosave: ["save_interval_seconds"], backups: ["backup_interval_seconds"] },
  terraria: { autosave: [], backups: ["worldrollbackstokeep"] },
  theforest: { autosave: ["autosave_interval_minutes"], backups: [] },
  unturned: { autosave: ["managed_save_interval_seconds"], backups: [] },
  valheim: { autosave: ["save_interval_seconds"], backups: ["backup_count", "backup_short_seconds", "backup_long_seconds"] },
  vrising: { autosave: ["autosave_interval_seconds"], backups: ["autosave_count", "autosave_smart_keep"] }
};

export function nativeSavePolicyKeys(moduleId: string): string[] {
  const policy = NATIVE_SAVE_POLICIES[moduleId];
  return policy ? [...policy.autosave, ...policy.backups] : [];
}

export function withMaintenanceSavePolicy(definition: SettingsModuleDefinition): SettingsModuleDefinition {
  const keys = nativeSavePolicyKeys(definition.id);
  if (keys.length === 0) return definition;
  const fieldPresentationOverrides = { ...definition.fieldPresentationOverrides };
  for (const key of keys) {
    fieldPresentationOverrides[key] = { ...fieldPresentationOverrides[key], owner: "maintenance" };
  }
  return { ...definition, fieldPresentationOverrides };
}
