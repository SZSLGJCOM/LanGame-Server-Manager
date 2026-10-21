import type { TranslateFn } from "../../i18n";
import type { RuntimeCommandDispatchOptions } from "../../types";

export interface GmCommandBuildResult {
  commands: string[];
  processKey: string | null;
  dispatchOptions: RuntimeCommandDispatchOptions;
  preview: string;
  error: string | null;
}

export type GmCommandBuilder = (
  values: Record<string, string>,
  t?: TranslateFn
) => GmCommandBuildResult;

export function commandResult(
  commands: string[],
  processKey: string | null,
  dispatchOptions: RuntimeCommandDispatchOptions
): GmCommandBuildResult {
  return {
    commands,
    processKey,
    dispatchOptions,
    preview: commands.join("\n"),
    error: null
  };
}

export function errorResult(error: string, dispatchOptions: RuntimeCommandDispatchOptions): GmCommandBuildResult {
  return {
    commands: [],
    processKey: null,
    dispatchOptions,
    preview: "",
    error
  };
}

export function readRequiredToken(
  values: Record<string, string>,
  key: string,
  label: string
): { value: string; error: string | null } {
  const value = String(values[key] ?? "").trim();
  if (!value) {
    return { value: "", error: `${label} is required.` };
  }
  if (/[\x00-\x1f\x7f]/.test(value)) {
    return { value: "", error: `${label} must be a single-line value.` };
  }
  return { value, error: null };
}

export function readRequiredLuaPrefab(
  values: Record<string, string>,
  key: string,
  label: string
): { value: string; error: string | null } {
  const token = readRequiredToken(values, key, label);
  if (token.error) {
    return token;
  }
  if (!/^[A-Za-z0-9_./-]+$/.test(token.value)) {
    return { value: "", error: `${label} can only contain letters, numbers, underscore, dash, slash, or dot.` };
  }
  return token;
}

/** DST prefab ids, from the catalog or typed by hand: lowercase ASCII letters, digits and underscores. */
export const DST_PREFAB_ID_PATTERN = /^[a-z0-9_]{1,64}$/;

export function readDstPrefabId(values: Record<string, string>, key: string, label: string): { value: string; error: string | null } {
  const token = readRequiredToken(values, key, label);
  if (token.error) return token;
  if (!DST_PREFAB_ID_PATTERN.test(token.value)) return { value: "", error: `${label} must be 1-64 lowercase letters, digits or underscores.` };
  return token;
}

export function readInteger(
  values: Record<string, string>,
  key: string,
  label: string,
  min: number,
  max: number
): { value: number; error: string | null } {
  const raw = String(values[key] ?? "").trim();
  if (!raw) {
    return { value: min, error: `${label} is required.` };
  }
  const value = Number(raw);
  if (!Number.isInteger(value)) {
    return { value: min, error: `${label} must be an integer.` };
  }
  if (value < min || value > max) {
    return { value: min, error: `${label} must be between ${min} and ${max}.` };
  }
  return { value, error: null };
}

export function readEnum(
  values: Record<string, string>,
  key: string,
  label: string,
  allowed: readonly string[]
): { value: string; error: string | null } {
  const value = String(values[key] ?? "").trim();
  if (!allowed.includes(value)) {
    return { value: "", error: `${label} is not supported.` };
  }
  return { value, error: null };
}

export function luaString(value: string): string {
  return JSON.stringify(value);
}
