import type { RuntimeResourceLimits } from "./types";

type Settings = Record<string, unknown>;
export interface ResourceDraft { cpu: string; memory: string; reserve: string }
const defaults: RuntimeResourceLimits = { cpu_percent: null, memory_limit_mib: null, host_memory_reserve_mib: 2048 };

function object(value: unknown, label: string): Settings {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label} must be an object`);
  return value as Settings;
}
function integer(value: unknown, min: number, max: number, name: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < min || value > max) {
    throw new Error(`${name} must be an integer from ${min} to ${max}`);
  }
  return value;
}
export function readResourceLimits(settings: Settings): RuntimeResourceLimits {
  if (settings.runtime_performance === undefined) return { ...defaults };
  const performance = object(settings.runtime_performance, "runtime_performance");
  if (performance.resource_limits === undefined) return { ...defaults };
  const value = object(performance.resource_limits, "resource_limits");
  for (const key of Object.keys(value)) if (!Object.prototype.hasOwnProperty.call(defaults, key)) throw new Error(`Unknown resource limit: ${key}`);
  return {
    cpu_percent: value.cpu_percent == null ? null : integer(value.cpu_percent, 1, 100, "cpu_percent"),
    memory_limit_mib: value.memory_limit_mib == null ? null : integer(value.memory_limit_mib, 64, 1048576, "memory_limit_mib"),
    host_memory_reserve_mib: value.host_memory_reserve_mib === undefined ? 2048
      : integer(value.host_memory_reserve_mib, 0, 1048576, "host_memory_reserve_mib")
  };
}
export function resourceDraft(limits: RuntimeResourceLimits): ResourceDraft {
  return { cpu: limits.cpu_percent === null ? "" : String(limits.cpu_percent),
    memory: limits.memory_limit_mib === null ? "" : String(limits.memory_limit_mib), reserve: String(limits.host_memory_reserve_mib) };
}
export function parseResourceDraft(draft: ResourceDraft): RuntimeResourceLimits {
  const parse = (value: string, min: number, max: number, label: string): number => {
    if (!/^\d+$/.test(value.trim())) throw new Error(label);
    try { return integer(Number(value), min, max, label); } catch { throw new Error(label); }
  };
  return { cpu_percent: draft.cpu.trim() ? parse(draft.cpu, 1, 100, "cpu") : null,
    memory_limit_mib: draft.memory.trim() ? parse(draft.memory, 64, 1048576, "memory") : null,
    host_memory_reserve_mib: parse(draft.reserve, 0, 1048576, "reserve") };
}
export function mergeResourceLimits(latest: Settings, baseline: Settings, limits: RuntimeResourceLimits): Settings {
  const current = readResourceLimits(latest);
  if (JSON.stringify(current) !== JSON.stringify(readResourceLimits(baseline))
    && JSON.stringify(current) !== JSON.stringify(limits)) throw new Error("runtime_resources_conflict");
  const performance = latest.runtime_performance === undefined ? {} : object(latest.runtime_performance, "runtime_performance");
  return { ...latest, runtime_performance: { ...performance, resource_limits: limits } };
}
