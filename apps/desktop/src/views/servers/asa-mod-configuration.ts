import type { SettingsObject } from "../settings/settings-schema";
import { canonicalAsaModId, readAsaModMembership } from "./mod-workbench-asa";

export const ASA_MOD_CONFIGURATION_KEY = "asa_mod_configurations";
export const ASA_MOD_INI_LIMIT = 64 * 1024;
export const ASA_MOD_CONFIGURATION_TOTAL_LIMIT = 1024 * 1024;
export const ASA_MOD_INI_FILES = { game_ini: "Game.ini", game_user_settings_ini: "GameUserSettings.ini" } as const;
export type AsaModIniFile = keyof typeof ASA_MOD_INI_FILES;
export interface AsaModConfiguration { game_ini: string; game_user_settings_ini: string }
export interface AsaModIniEntry { section: string; key: string; value: string; line: number }
export type AsaModConfigurationIssueCode = "invalid-id" | "shape" | "size" | "total-size" | "count"
  | "encoding" | "file-type" | "section" | "assignment" | "control" | "conflict" | "base-changed" | "duplicate-parameter";
export interface AsaModConfigurationIssue {
  code: AsaModConfigurationIssueCode;
  severity: "error" | "warning";
  line?: number; section?: string; key?: string; owner?: string;
}
export class AsaModConfigurationError extends Error {
  constructor(readonly issue: AsaModConfigurationIssue) { super(issue.code); }
}
export type AsaModLocalizedText = string | Readonly<Partial<Record<"zh-CN" | "en-US", string>>>;
export interface AsaModParameterDefinition {
  file: AsaModIniFile; section: string; key: string;
  label: AsaModLocalizedText; description?: AsaModLocalizedText;
  kind: "text" | "number" | "boolean" | "select";
  defaultValue?: string; trueValue?: string; falseValue?: string;
  choices?: readonly { value: string; label: AsaModLocalizedText }[];
  requires?: { file: AsaModIniFile; section: string; key: string; value: string };
}
export interface AsaModAuthorDefinition {
  modId: string; title?: string; sourceUrl?: string; parameters: readonly AsaModParameterDefinition[];
}
/** Only verified author keys belong here; unknown Mods use the same INI editor. */
export const ASA_MOD_AUTHOR_CATALOG: readonly AsaModAuthorDefinition[] = [{
  modId: "1254808", title: "Better Variants",
  sourceUrl: "https://www.curseforge.com/ark-survival-ascended/mods/better-variants",
  parameters: [
    { file: "game_user_settings_ini", section: "BetterVariants", key: "Debug", kind: "boolean",
      label: { "en-US": "Debug logging", "zh-CN": "调试日志" }, defaultValue: "False", trueValue: "True", falseValue: "False" },
    { file: "game_user_settings_ini", section: "BetterVariants", key: "DebugSpawns", kind: "boolean",
      label: { "en-US": "Spawn debug logging", "zh-CN": "生物生成调试日志" }, defaultValue: "False", trueValue: "True", falseValue: "False",
      requires: { file: "game_user_settings_ini", section: "BetterVariants", key: "Debug", value: "True" } },
  ],
}];
const lower = (text: string) => text.replace(/[A-Z]/g, (letter) => letter.toLowerCase());
const bytes = (text: string) => new TextEncoder().encode(text).length;
const record = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value);
const fail = (code: AsaModConfigurationIssueCode): never => { throw new AsaModConfigurationError({ code, severity: "error" }); };

export function readAsaModConfigurations(settings: Readonly<SettingsObject>): Record<string, AsaModConfiguration> {
  const raw = settings[ASA_MOD_CONFIGURATION_KEY];
  if (raw === undefined) return {};
  if (!record(raw)) return fail("shape");
  const entries = Object.entries(raw);
  if (entries.length > 256) return fail("count");
  let total = 0;
  return Object.fromEntries(entries.map(([id, value]) => {
    if (canonicalAsaModId(id) !== id) return fail("invalid-id");
    if (!record(value) || Object.keys(value).some((key) => !Object.keys(ASA_MOD_INI_FILES).includes(key))
      || Object.keys(ASA_MOD_INI_FILES).some((key) => value[key] !== undefined && typeof value[key] !== "string")) return fail("shape");
    const config = { game_ini: String(value.game_ini ?? ""), game_user_settings_ini: String(value.game_user_settings_ini ?? "") };
    for (const text of Object.values(config)) {
      const length = bytes(text); total += length;
      if (length > ASA_MOD_INI_LIMIT) return fail("size");
      if (total > ASA_MOD_CONFIGURATION_TOTAL_LIMIT) return fail("total-size");
    }
    return [id, config];
  }));
}

export function parseAsaModIni(raw: string) {
  const text = raw.replace(/^\uFEFF/, "");
  const entries: AsaModIniEntry[] = [], issues: AsaModConfigurationIssue[] = [];
  if (bytes(text) > ASA_MOD_INI_LIMIT) return { text, entries, issues: [{ code: "size", severity: "error" } as AsaModConfigurationIssue], isClear: false };
  let section = "";
  text.replace(/\r\n/g, "\n").split("\n").forEach((line, index) => {
    const trimmed = line.trim(), lineNumber = index + 1;
    if (/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/.test(line)) {
      issues.push({ code: "control", severity: "error", line: lineNumber }); return;
    }
    if (!trimmed || /^(?:;|#|\/\/)/.test(trimmed)) return;
    if (trimmed.startsWith("[")) {
      const header = trimmed.match(/^\[([^\[\]]+)\]\s*(?:(?:;|#|\/\/).*)?$/);
      section = header?.[1].trim() ?? "";
      if (!section) issues.push({ code: "section", severity: "error", line: lineNumber });
      return;
    }
    const equals = line.indexOf("="), key = equals >= 0 ? line.slice(0, equals).trim() : "";
    if (!section || !key) { issues.push({ code: "assignment", severity: "error", line: lineNumber }); return; }
    entries.push({ section, key, value: line.slice(equals + 1).trim(), line: lineNumber });
  });
  return { text, entries, issues, isClear: text.trim().length === 0 };
}

export function decodeAsaModIniFile(input: ArrayBuffer | Uint8Array): string {
  const data = input instanceof Uint8Array ? input : new Uint8Array(input);
  if (data.byteLength > ASA_MOD_INI_LIMIT) return fail("size");
  let encoding = "utf-8", offset = 0;
  if (data[0] === 0xff && data[1] === 0xfe) { encoding = "utf-16le"; offset = 2; }
  else if (data[0] === 0xfe && data[1] === 0xff) { encoding = "utf-16be"; offset = 2; }
  else if (data[0] === 0xef && data[1] === 0xbb && data[2] === 0xbf) offset = 3;
  try {
    const text = new TextDecoder(encoding, { fatal: true }).decode(data.subarray(offset));
    if (bytes(text) > ASA_MOD_INI_LIMIT) return fail("size");
    return text;
  } catch (error) {
    if (error instanceof AsaModConfigurationError) throw error;
    return fail("encoding");
  }
}

const identity = (entry: Pick<AsaModIniEntry, "section" | "key">) => `${lower(entry.section)}\u0000${lower(entry.key)}`;
const repeatedNativeRule = (file: AsaModIniFile, entry: AsaModIniEntry) => file === "game_ini"
  && lower(entry.section) === "/script/shootergame.shootergamemode"
  && ["configoverrideitemcraftingcosts", "configoverrideitemmaxquantity", "overridenamedengramentries"].includes(lower(entry.key));

export function findAsaModIniConflicts(settings: Readonly<SettingsObject>, modId: string, file: AsaModIniFile,
  entries: readonly AsaModIniEntry[]): AsaModConfigurationIssue[] {
  const configs = readAsaModConfigurations(settings), enabled = readAsaModMembership(settings).enabled;
  const targets = new Map(entries.filter((entry) => !repeatedNativeRule(file, entry)).map((entry) => [identity(entry), entry]));
  const result: AsaModConfigurationIssue[] = [], seen = new Set<string>();
  const inspect = (text: string, owner: string, active: boolean) => {
    for (const other of parseAsaModIni(text).entries) {
      const entry = targets.get(identity(other)), claim = `${owner}\u0000${identity(other)}`;
      if (!entry || seen.has(claim)) continue;
      seen.add(claim); result.push({ code: "conflict", severity: enabled.has(modId) && active ? "error" : "warning",
        section: entry.section, key: entry.key, line: entry.line, owner });
    }
  };
  for (const [otherId, config] of Object.entries(configs)) {
    if (otherId !== modId) inspect(config[file], `Mod ${otherId}`, enabled.has(otherId));
  }
  const rawKey = file === "game_ini" ? "game_ini_extra" : "game_user_settings_extra";
  const raw = settings[rawKey];
  if (typeof raw === "string") inspect(`[${file === "game_ini" ? "/script/shootergame.shootergamemode" : "messageoftheday"}]\n${raw}`, rawKey, true);
  return result;
}

/** Save exactly the reviewed destination, preserving every other Mod and file. */
export function buildAsaModConfigurationSettings(settings: Readonly<SettingsObject>, modId: string,
  file: AsaModIniFile, raw: string, expectedText: string): SettingsObject {
  if (canonicalAsaModId(modId) !== modId) return fail("invalid-id");
  const configs = readAsaModConfigurations(settings);
  const current = configs[modId] ?? { game_ini: "", game_user_settings_ini: "" };
  if (current[file] !== expectedText) return fail("base-changed");
  const parsed = parseAsaModIni(raw);
  const error = [...parsed.issues, ...findAsaModIniConflicts(settings, modId, file, parsed.entries)].find((issue) => issue.severity === "error");
  if (error) throw new AsaModConfigurationError(error);
  const next = { ...current, [file]: parsed.isClear ? "" : parsed.text };
  const nextConfigs = { ...configs };
  if (!next.game_ini && !next.game_user_settings_ini) delete nextConfigs[modId];
  else nextConfigs[modId] = next;
  const result = { ...settings, [ASA_MOD_CONFIGURATION_KEY]: nextConfigs };
  readAsaModConfigurations(result);
  return result;
}

export function asaModParameterAssignments(text: string, parameter: AsaModParameterDefinition): AsaModIniEntry[] {
  return parseAsaModIni(text).entries.filter((entry) => identity(entry) === identity(parameter));
}

export function asaModParameterValue(text: string, parameter: AsaModParameterDefinition): string | undefined {
  const matching = asaModParameterAssignments(text, parameter);
  return matching.length === 1 ? matching[0].value : undefined;
}

/** Adapted fields edit the one authoritative INI string, never a second options map. */
export function setAsaModIniParameter(text: string, parameter: AsaModParameterDefinition, value: string | null): string {
  const parsed = parseAsaModIni(text);
  if (parsed.issues.length) throw new AsaModConfigurationError(parsed.issues[0]);
  const matching = parsed.entries.filter((entry) => identity(entry) === identity(parameter));
  if (matching.length > 1) return fail("duplicate-parameter");
  if (value !== null && /[\r\n\u0000-\u001f\u007f-\u009f\u2028\u2029]/.test(value)) return fail("control");
  const newline = parsed.text.includes("\r\n") ? "\r\n" : "\n";
  const lines = parsed.text.replace(/\r\n/g, "\n").split("\n");
  if (matching.length) {
    const index = matching[0].line - 1;
    if (value === null) lines.splice(index, 1);
    else lines[index] = `${matching[0].key}=${value}`;
    return lines.join(newline);
  }
  if (value === null) return parsed.text;
  let sectionIndex = -1;
  for (let index = 0; index < lines.length; index++) {
    const header = lines[index].trim().match(/^\[([^\[\]]+)\]/);
    if (header && lower(header[1].trim()) === lower(parameter.section)) sectionIndex = index;
  }
  if (sectionIndex >= 0) lines.splice(sectionIndex + 1, 0, `${parameter.key}=${value}`);
  else {
    while (lines.length && !lines[lines.length - 1].trim()) lines.pop();
    if (lines.length) lines.push("");
    lines.push(`[${parameter.section}]`, `${parameter.key}=${value}`, "");
  }
  return lines.join(newline);
}

export function localizedAsaModText(text: AsaModLocalizedText | undefined, locale: string): string {
  if (typeof text === "string") return text;
  return text?.[locale === "zh-CN" ? "zh-CN" : "en-US"] ?? text?.["en-US"] ?? text?.["zh-CN"] ?? "";
}
