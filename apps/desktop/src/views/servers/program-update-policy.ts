import type { ModuleDetails } from "../../types";
import type { SettingsObject } from "../settings/settings-schema";

export type ProgramUpdatePolicy = "automatic" | "pinned";

function object(value: unknown): SettingsObject | null {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as SettingsObject : null;
}

export function readProgramUpdatePolicy(settings: SettingsObject): ProgramUpdatePolicy {
  if (settings.program_update === undefined) return "automatic";
  const section = object(settings.program_update);
  if (!section || Object.keys(section).some((key) => key !== "policy")) throw new Error("program_update_invalid");
  const policy = section.policy;
  if (policy === undefined) return "automatic";
  if (policy !== "automatic" && policy !== "pinned") throw new Error("program_update_invalid");
  return policy;
}

export function supportsProgramUpdates(module: ModuleDetails): boolean {
  return Boolean(module.install && module.install.download_url_windows == null
    && ((module.summary.steam_app_id ?? 0) > 0 || module.install.source === "minecraft_java"));
}

export function mergeProgramUpdatePolicy(
  latest: SettingsObject, baseline: SettingsObject, policy: ProgramUpdatePolicy
): SettingsObject {
  const current = readProgramUpdatePolicy(latest);
  if (readProgramUpdatePolicy(baseline) !== current && current !== policy) {
    throw new Error("program_update_conflict");
  }
  return { ...latest, program_update: { ...object(latest.program_update), policy } };
}
