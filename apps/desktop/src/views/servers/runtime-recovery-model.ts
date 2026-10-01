import type { SettingsObject } from "../settings/settings-schema";

export interface RecoveryPolicy {
  enabled: boolean;
  max_restarts: number;
  backoff_ms: number;
  only_nonzero_exit: boolean;
}
export interface RecoveryDraft {
  enabled: boolean;
  maxRestarts: string;
  waitSeconds: string;
  onlyNonzeroExit: boolean;
}

function object(value: unknown): SettingsObject {
  return value && typeof value === "object" && !Array.isArray(value) ? value as SettingsObject : {};
}
function booleanValue(value: unknown): boolean | undefined {
  return typeof value === "boolean" ? value : undefined;
}
function integerValue(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && Number.isInteger(value) && value >= 0 ? value : undefined;
}

export function readRecoveryPolicy(settings: SettingsObject): RecoveryPolicy {
  const native = object(settings.runtime_restart);
  return {
    enabled: booleanValue(native.enabled) ?? false,
    max_restarts: Math.max(1, Math.min(10, integerValue(native.max_restarts) ?? 3)),
    backoff_ms: Math.min(300_000, integerValue(native.backoff_ms) ?? 5000),
    only_nonzero_exit: booleanValue(native.only_nonzero_exit) ?? true
  };
}
export function recoveryDraft(policy: RecoveryPolicy): RecoveryDraft {
  return { enabled: policy.enabled, maxRestarts: String(policy.max_restarts),
    waitSeconds: String(policy.backoff_ms / 1000), onlyNonzeroExit: policy.only_nonzero_exit };
}
export function parseRecoveryDraft(draft: RecoveryDraft):
  { value: RecoveryPolicy; error: null } | { value: null; error: "count" | "wait" } {
  const maxRestarts = Number(draft.maxRestarts);
  if (!/^\d+$/.test(draft.maxRestarts.trim()) || maxRestarts < 1 || maxRestarts > 10) return { value: null, error: "count" };
  const seconds = Number(draft.waitSeconds);
  if (!/^\d+(?:\.\d{1,3})?$/.test(draft.waitSeconds.trim()) || seconds < 0 || seconds > 300) return { value: null, error: "wait" };
  return { value: { enabled: draft.enabled, max_restarts: maxRestarts, backoff_ms: Math.round(seconds * 1000), only_nonzero_exit: draft.onlyNonzeroExit }, error: null };
}
export function recoveryDraftChanged(draft: RecoveryDraft, baseline: SettingsObject): boolean {
  return JSON.stringify(parseRecoveryDraft(draft).value) !== JSON.stringify(readRecoveryPolicy(baseline));
}
export function mergeRecoveryPolicy(latest: SettingsObject, baseline: SettingsObject, policy: RecoveryPolicy): SettingsObject {
  const next = { ...latest, runtime_restart: { ...object(latest.runtime_restart), ...policy } };
  const latestPolicy = JSON.stringify(readRecoveryPolicy(latest));
  if (JSON.stringify(readRecoveryPolicy(baseline)) !== latestPolicy
    && JSON.stringify(readRecoveryPolicy(next)) !== latestPolicy) throw new Error("runtime_recovery_conflict");
  return next;
}
