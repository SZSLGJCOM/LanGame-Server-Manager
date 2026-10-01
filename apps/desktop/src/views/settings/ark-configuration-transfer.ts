import type { ConfigurationPresentationField, SettingsObject } from "./settings-schema";
import { validateArkComplexField } from "./ark-complex-validation";

export const ARK_IMPORT_MAX_BYTES = 2 * 1024 * 1024;
export interface ArkIniDocument { name: string; text: string }
export interface ArkImportIssue { document: string; line: number; key: string; reason: "syntax" | "value" }
export interface ArkIniImport {
  patch: SettingsObject;
  issues: ArkImportIssue[];
  skippedKeys: string[];
  unknownCount: number;
}
type Properties = Record<string, Record<string, unknown>>;
interface NativeField { key: string; section: string; native: RegExp; spelling: string; type: unknown; repeated: boolean }

const GAME_MODE = "/script/shootergame.shootergamemode";
const UNSAFE_KEYS = new Set(["__proto__", "prototype", "constructor"]);
const MANAGED_KEYS = new Set(["port", "queryport", "rconport", "multihome", "altsavedirectoryname", "version", "activemods"]);
const NON_PORTABLE_SECTIONS = new Set(["room", "network", "access", "admin", "join", "moderation", "transfer", "advanced", "mods", "operations", "logs"]);
const NON_PORTABLE_KEYS = /password|secret|token|credential|account|admin_ids|launch_flags|_extra$|cluster_|additional_maps|mod_ids|auto_managed_mod|map_name|server_name|server_platform|exclusive_join|priority_join/i;
const hasOwn = (value: object, key: string) => Object.prototype.hasOwnProperty.call(value, key);

function assertSize(text: string) {
  if (new TextEncoder().encode(text).byteLength > ARK_IMPORT_MAX_BYTES) throw new Error("Configuration file exceeds the size limit (2 MiB).");
}

function nativeFields(properties: Properties, gameIni: boolean): NativeField[] {
  return Object.entries(properties).flatMap(([key, property]) => {
    const source = property["x-lsgm-source"];
    const sourceKey = property["x-lsgm-source-key"];
    if (UNSAFE_KEYS.has(key) || typeof sourceKey !== "string" || sourceKey === "raw_extra_lines") return [];
    if (gameIni ? source !== "game_ini" && source !== "asa_advanced_game_ini" : source !== "game_user_settings") return [];
    const separator = sourceKey.lastIndexOf(".");
    if (separator < 0) return [];
    const section = sourceKey.slice(0, separator).replace(/^\[|\]$/g, "").toLowerCase();
    const spelling = sourceKey.slice(separator + 1);
    const keyPattern = spelling.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
      .replace(/<(?:integer|attribute|Stat_ID)>/gi, "(\\d+)").replace(/<_type>/g, "(_Add|_Affinity)?");
    const field = { key, section, spelling, native: new RegExp(`^${keyPattern}$`, "i"), type: property.type,
      repeated: property.format === "textarea" || /<[^>]+>/.test(spelling) };
    return key === "max_players" ? [field, { ...field, section: section === "sessionsettings" ? "/script/engine.gamesession" : "sessionsettings" }] : [field];
  });
}

function scalarValue(text: string, type: unknown): unknown {
  if (type === "string") {
    return text.startsWith('"') && text.endsWith('"') ? text.slice(1, -1) : text;
  }
  const scalar = text.replace(/\s+[;#].*$/, "").trim();
  if (type === "boolean") {
    if (/^(true|1)$/i.test(scalar)) return true;
    if (/^(false|0)$/i.test(scalar)) return false;
    return undefined;
  }
  if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?$/i.test(scalar)) return undefined;
  const value = Number(scalar);
  return Number.isFinite(value) && (type !== "integer" || Number.isSafeInteger(value)) ? value : undefined;
}

function validFieldValue(key: string, value: unknown, property: Record<string, unknown>): boolean {
  const type = property.type;
  if (typeof value !== (type === "integer" ? "number" : type)) return false;
  if (typeof value === "number" && (!Number.isFinite(value) || type === "integer" && !Number.isSafeInteger(value)
    || typeof property.minimum === "number" && value < property.minimum
    || typeof property.maximum === "number" && value > property.maximum)) return false;
  if (typeof value === "string" && (
    typeof property.maxLength === "number" && value.length > property.maxLength
    || typeof property.minLength === "number" && value.length < property.minLength
    || typeof property.pattern === "string" && !new RegExp(property.pattern).test(value)
    || validateArkComplexField(key, value))) return false;
  return !Array.isArray(property.enum) || property.enum.includes(value);
}

/** Files are parsed in memory; applying the resulting patch uses the normal instance save queue. */
export function importArkIniDocuments(properties: Properties, documents: readonly ArkIniDocument[]): ArkIniImport {
  const result: ArkIniImport = { patch: {}, issues: [], skippedKeys: [], unknownCount: 0 };
  const seen = new Set<string>();
  const origins = new Map<string, Omit<ArkImportIssue, "reason">>();
  for (const document of documents) {
    assertSize(document.text);
    const name = document.name.toLowerCase();
    if (!["game.ini", "gameusersettings.ini"].includes(name) || seen.has(name)) {
      result.issues.push({ document: document.name, line: 0, key: document.name, reason: "syntax" });
      continue;
    }
    seen.add(name);
    const gameIni = name === "game.ini";
    const fields = nativeFields(properties, gameIni);
    const extras: string[] = [];
    let section = gameIni ? GAME_MODE : "serversettings";
    let heading = `[${section}]`;
    let lastExtraSection: string | undefined;
    const appendExtra = (line: string) => {
      if (lastExtraSection !== section) { extras.push(heading); lastExtraSection = section; }
      extras.push(line);
    };
    document.text.replace(/^\uFEFF/, "").split(/\r?\n/).forEach((line, index) => {
      const trimmed = line.trim();
      if (!trimmed) return;
      if (/^[;#]/.test(trimmed)) { appendExtra(line); return; }
      if (trimmed.startsWith("[")) {
        if (!/^\[[^\]\r\n]+\]\s*(?:[;#].*)?$/.test(trimmed) || !trimmed.slice(1, trimmed.indexOf("]")).trim()) {
          result.issues.push({ document: document.name, line: index + 1, key: "", reason: "syntax" });
          return;
        }
        heading = trimmed.slice(0, trimmed.indexOf("]") + 1);
        section = heading.slice(1, -1).trim().toLowerCase();
        return;
      }
      const equals = line.indexOf("=");
      if (equals < 1 || /[\u0000-\u0008\u000B\u000C\u000E-\u001F]/.test(line)) {
        result.issues.push({ document: document.name, line: index + 1, key: "", reason: "syntax" });
        return;
      }
      const nativeKey = line.slice(0, equals).trim();
      const rawValue = line.slice(equals + 1).trim();
      if (["serversettings", "sessionsettings", "/script/engine.gamesession", "/script/shootergame.shootergameusersettings"].includes(section) && MANAGED_KEYS.has(nativeKey.replace(/^[+.!-]+/, "").toLowerCase())) {
        result.skippedKeys.push(nativeKey);
        return;
      }
      const field = fields.find((candidate) => candidate.section === section && candidate.native.test(nativeKey));
      if (!field) { appendExtra(line); result.unknownCount++; return; }
      const captures = field.native.exec(nativeKey)?.slice(1) ?? [];
      let capture = 0;
      const canonicalNativeKey = field.spelling.replace(/<[^>]+>/g, (placeholder) => {
        const value = captures[capture++] ?? "";
        return placeholder === "<_type>" ? /^_add$/i.test(value) ? "_Add" : value ? "_Affinity" : "" : value;
      });
      const value = field.repeated ? `${canonicalNativeKey}=${rawValue}` : scalarValue(rawValue, field.type);
      if (value === undefined) {
        result.issues.push({ document: document.name, line: index + 1, key: nativeKey, reason: "value" });
        return;
      }
      const previous = result.patch[field.key];
      result.patch[field.key] = field.repeated && typeof previous === "string" ? `${previous}\n${String(value)}` : value;
      origins.set(field.key, { document: document.name, line: index + 1, key: nativeKey });
    });
    if (extras.length) result.patch[gameIni ? "game_ini_extra" : "game_user_settings_extra"] = extras.join("\n");
  }
  for (const [key, origin] of origins) {
    if (!validFieldValue(key, result.patch[key], properties[key])) result.issues.push({ ...origin, reason: "value" });
  }
  result.skippedKeys = [...new Set(result.skippedKeys)];
  return result;
}

export interface ArkPreset { format: "lgsm-ark-preset"; moduleId: string; settings: SettingsObject; reset: string[] }
export function isPortableArkField(field: ConfigurationPresentationField): boolean {
  return field.presentation.owner === "configuration" && !NON_PORTABLE_SECTIONS.has(field.sectionId)
    && !["secret", "path", "raw"].includes(field.presentation.behavior ?? "") && !NON_PORTABLE_KEYS.test(field.key);
}

export function createArkPreset(moduleId: string, settings: Readonly<SettingsObject>, fields: readonly ConfigurationPresentationField[], sections: readonly string[]): ArkPreset {
  const allowed = new Set(sections);
  const selected = fields.filter((field) => allowed.has(field.sectionId) && isPortableArkField(field));
  const entries = selected.filter((field) => hasOwn(settings, field.key) && settings[field.key] !== undefined);
  return { format: "lgsm-ark-preset", moduleId, settings: Object.fromEntries(entries.map((field) => [field.key, settings[field.key]])),
    reset: selected.filter((field) => !hasOwn(settings, field.key) || settings[field.key] === undefined).map((field) => field.key) };
}

export function readArkPreset(text: string, moduleId: string, properties: Properties): SettingsObject {
  assertSize(text);
  const parsed: unknown = JSON.parse(text);
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("Invalid ARK preset.");
  const data = parsed as Record<string, unknown>;
  if (data.format !== "lgsm-ark-preset" || data.moduleId !== moduleId) throw new Error("The preset belongs to a different ARK edition.");
  if (!data.settings || typeof data.settings !== "object" || Array.isArray(data.settings)) throw new Error("Invalid preset settings.");
  const validateKey = (key: string) => {
    if (UNSAFE_KEYS.has(key) || NON_PORTABLE_KEYS.test(key) || !hasOwn(properties, key)
      || NON_PORTABLE_SECTIONS.has(String(properties[key]["x-lsgm-section"]))) throw new Error(`Unknown or private preset field: ${key}`);
  };
  for (const [key, value] of Object.entries(data.settings)) {
    validateKey(key);
    if (!validFieldValue(key, value, properties[key])) {
      throw new Error(`Invalid preset field: ${key}`);
    }
  }
  const result: SettingsObject = { ...data.settings };
  if (data.reset !== undefined) {
    if (!Array.isArray(data.reset) || !data.reset.every((key) => typeof key === "string")) throw new Error("Invalid preset reset fields.");
    for (const key of data.reset) {
      validateKey(key);
      if (hasOwn(result, key)) throw new Error(`Conflicting preset field: ${key}`);
      result[key] = undefined;
    }
  }
  return result;
}
