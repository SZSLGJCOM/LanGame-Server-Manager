import type { SettingsObject } from "../settings/settings-schema";
import { parseAsaLaunchFlags } from "./mod-workbench-asa";

export type AsaModCompatibilityMode = "crossplay" | "steam";
const option = (value: string) => value.split("=")[0].toLowerCase();

export function readAsaModCompatibility(settings: SettingsObject) {
  const raw = typeof settings.custom_launch_flags === "string" ? settings.custom_launch_flags : "";
  const parsed = parseAsaLaunchFlags(raw);
  const platform = typeof settings.server_platform === "string" ? settings.server_platform.trim().toUpperCase() : "";
  const pcOnly = parsed.tokens.some(({ value }) => option(value) === "-pconlymods");
  const problem = !parsed.balanced ? "invalidFlags"
    : parsed.tokens.some(({ value }) => option(value) === "-serverplatform") ? "rawPlatform" : null;
  const mode: AsaModCompatibilityMode | "custom" = problem ? "custom"
    : pcOnly && platform === "PC" ? "steam"
      : !pcOnly && (!platform || platform === "ALL") ? "crossplay" : "custom";
  return { mode, problem, raw, tokens: parsed.tokens };
}

/** One explicit platform choice updates the two native options together; Mod lists stay intact. */
export function buildAsaModCompatibilityPlan(settings: SettingsObject, mode: AsaModCompatibilityMode): SettingsObject {
  const current = readAsaModCompatibility(settings);
  if (current.problem) throw new Error(current.problem);
  if (mode !== "crossplay" && mode !== "steam") throw new Error("invalidMode");
  let flags = "", offset = 0;
  for (const token of current.tokens) {
    if (option(token.value) !== "-pconlymods") continue;
    flags += current.raw.slice(offset, token.start);
    offset = token.end;
  }
  flags += current.raw.slice(offset);
  if (mode === "steam") flags += `${flags && !/\p{White_Space}$/u.test(flags) ? " " : ""}-pconlymods`;
  return { ...settings, server_platform: mode === "steam" ? "PC" : "ALL", custom_launch_flags: flags };
}
