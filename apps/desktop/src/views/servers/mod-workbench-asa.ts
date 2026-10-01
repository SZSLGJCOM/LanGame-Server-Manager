import type { SettingsObject } from "../settings/settings-schema";

export const ASA_MEMBERSHIP_KEYS = ["mod_ids_csv", "passive_mod_ids_csv", "curseforge_disabled_mod_ids",
  "curseforge_disabled_passive_mod_ids", "curseforge_removed_mod_ids", "custom_launch_flags"] as const;
export type AsaModAction = "enable" | "disable" | "remove" | "add" | "restore-files";
export const ASA_RAW_MOD_WARNING = "Custom launch flags define Mod loading. Remove -mods and -passivemods there before using these controls.";

/** Keep token boundaries and quoting aligned with native parse_custom_launch_flags. */
export function hasAsaRawModFlags(settings: SettingsObject): boolean {
  const raw = typeof settings.custom_launch_flags === "string" ? settings.custom_launch_flags : "";
  const arguments_: string[] = [];
  let current = "", single = false, double = false;
  for (let index = 0; index < raw.length; index += 1) {
    const character = raw[index];
    if (character === '"' && !single) double = !double;
    else if (character === "'" && !double) single = !single;
    else if (character === "\\" && double && (raw[index + 1] === '"' || raw[index + 1] === "\\")) current += raw[++index];
    else if (/\p{White_Space}/u.test(character) && !single && !double) {
      if (current) arguments_.push(current);
      current = "";
    } else current += character;
  }
  if (current) arguments_.push(current);
  return arguments_.some((argument) => ["-mods", "-passivemods"].includes(argument.split("=")[0].toLowerCase()));
}

export function canonicalAsaModId(value: unknown): string | null {
  if (typeof value !== "string" || !/^\d{1,20}$/.test(value)) return null;
  const id = BigInt(value);
  return id > 0n && id <= 18446744073709551615n ? id.toString() : null;
}

const unique = (values: string[]) => [...new Set(values)];
const trimCandidate = (value: string) => value.trim().replace(/^["'`\[\]()<>{}.]+|["'`\[\]()<>{}.]+$/g, "");
const tokens = (value: unknown) => typeof value === "string" ? value.split(/[,;\s、，；]+/).filter(Boolean) : [];

/** Match the native ASA launch parser's numeric, prefixed and URL reference forms. */
export function parseAsaModIds(value: unknown): string[] {
  const ids: string[] = [];
  const add = (candidate: string) => { const id = canonicalAsaModId(trimCandidate(candidate)); if (id) ids.push(id); };
  for (const raw of tokens(value)) {
    const token = trimCandidate(raw);
    add(token);
    const prefixed = token.match(/^(?:cf[-:]|curseforge[-:]|project[-:]|mod[-:])(.+)$/i);
    if (prefixed) add(prefixed[1]);
    for (const pair of (token.includes("?") ? token.slice(token.indexOf("?") + 1) : token).split("&")) {
      const equal = pair.indexOf("=");
      if (equal >= 0 && /^(id|modid|projectid)$/i.test(pair.slice(0, equal).replace(/[^a-z0-9]/gi, ""))) add(pair.slice(equal + 1));
    }
    const segments = token.split("?")[0].split(/[\\/]/).map(trimCandidate);
    segments.forEach((segment, index) => { if (index > 0 && /^(mods?|projects?)$/i.test(segments[index - 1])) add(segment); });
  }
  return unique(ids);
}

function markerIds(settings: SettingsObject, key: string): string[] {
  const value = settings[key];
  return Array.isArray(value) && value.length <= 8192
    ? unique(value.filter((id): id is string => typeof id === "string" && canonicalAsaModId(id) === id)) : [];
}

export function readAsaModMembership(settings: SettingsObject) {
  const active = parseAsaModIds(settings.mod_ids_csv);
  const passive = parseAsaModIds(settings.passive_mod_ids_csv);
  // An explicit native setting takes precedence over a previous removal marker.
  const enabled = new Set([...active, ...passive]);
  const removed = new Set(markerIds(settings, "curseforge_removed_mod_ids").filter((id) => !enabled.has(id)));
  const disabledActive = markerIds(settings, "curseforge_disabled_mod_ids").filter((id) => !removed.has(id));
  const disabledPassive = markerIds(settings, "curseforge_disabled_passive_mod_ids").filter((id) => !removed.has(id));
  const disabled = unique([...disabledActive, ...disabledPassive]).filter((id) => !enabled.has(id));
  return { active, passive, enabled, disabledActive, disabledPassive, disabled, removed,
    owned: new Set([...enabled, ...disabled]) };
}

export class AsaModControlError extends Error {
  constructor(readonly code: "invalid-id" | "not-owned" | "raw-mod-flags") { super(code); }
}

/** Change only ASA loading/ownership fields; retained files and unrelated options are untouched. */
export function buildAsaModPlan(settings: SettingsObject, rawIds: readonly string[], action: AsaModAction,
  inventoryIds: readonly string[] = []): SettingsObject {
  if (hasAsaRawModFlags(settings)) throw new AsaModControlError("raw-mod-flags");
  const ids = unique(rawIds.map((id) => { const normalized = canonicalAsaModId(id); if (!normalized) throw new AsaModControlError("invalid-id"); return normalized; }));
  if (!ids.length || ids.length > 8192) throw new AsaModControlError("invalid-id");
  const state = readAsaModMembership(settings);
  const local = new Set(inventoryIds.map(canonicalAsaModId).filter((id): id is string => id !== null));
  if (action !== "add" && action !== "restore-files" && ids.some((id) =>
    !state.owned.has(id) && (!local.has(id) || state.removed.has(id)))) throw new AsaModControlError("not-owned");
  const target = new Set(ids), next = { ...settings };
  const disabledActive = new Set(state.disabledActive), disabledPassive = new Set(state.disabledPassive);
  const removed = new Set(markerIds(settings, "curseforge_removed_mod_ids"));
  const addActive: string[] = [], addPassive: string[] = [];
  for (const id of ids) {
    if (action === "disable") {
      if (state.enabled.has(id)) { disabledActive.delete(id); disabledPassive.delete(id); }
      if (state.active.includes(id) || (!state.passive.includes(id) && !disabledPassive.has(id))) disabledActive.add(id);
      if (state.passive.includes(id)) disabledPassive.add(id);
      removed.delete(id);
    } else if (action === "remove") {
      disabledActive.delete(id); disabledPassive.delete(id); removed.add(id);
    } else if (action === "restore-files") {
      // Copying files restores visibility without silently enabling gameplay.
      removed.delete(id);
      if (!state.owned.has(id)) disabledActive.add(id);
    } else {
      if (action === "add" || disabledActive.has(id) || (!disabledPassive.has(id) && !state.enabled.has(id))) addActive.push(id);
      if (action === "enable" && disabledPassive.has(id)) addPassive.push(id);
      disabledActive.delete(id); disabledPassive.delete(id); removed.delete(id);
    }
  }
  for (const [key, additions] of [["mod_ids_csv", addActive], ["passive_mod_ids_csv", addPassive]] as const) {
    const existing = tokens(settings[key]);
    const remaining = action === "disable" || action === "remove" ? existing.flatMap((token) => {
      const parsed = parseAsaModIds(token);
      return parsed.some((id) => target.has(id)) ? parsed.filter((id) => !target.has(id)) : [token];
    }) : existing;
    const existingIds = new Set(parseAsaModIds(remaining.join("\n")));
    const appended = additions.filter((id) => !existingIds.has(id));
    if (remaining.length !== existing.length || remaining.some((token, index) => token !== existing[index]) || appended.length) {
      next[key] = [...remaining, ...appended].join("\n");
    }
  }
  for (const [key, values] of [["curseforge_disabled_mod_ids", disabledActive],
    ["curseforge_disabled_passive_mod_ids", disabledPassive], ["curseforge_removed_mod_ids", removed]] as const) {
    if (values.size > 8192) throw new AsaModControlError("invalid-id");
    if (values.size || key in settings) next[key] = [...values];
  }
  return next;
}
