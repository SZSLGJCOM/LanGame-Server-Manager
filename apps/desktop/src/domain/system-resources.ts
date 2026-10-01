import type { SystemSnapshot } from "../types";
import type { SystemTelemetry, TelemetrySampleStatus } from "../system-resource-types";
import { displayVolumePath } from "./system-volume-label";

export type ResourceState = "normal" | "watch" | "critical" | "unknown" | "stale";
export type ResourceChannel = Exclude<keyof SystemTelemetry, "observed_at_unix_ms">;
export type ResourceIssueCode = "cpu_load" | "cpu_core_load" | "memory_available" | "memory_commit" | "disk_space" | "disk_latency";
export interface ResourceIssue {
  code: ResourceIssueCode;
  severity: "watch" | "critical";
  value: number;
  threshold: number;
  target?: string;
}
export interface ResourceVolume {
  id: string;
  label: string;
  paths: string[];
  totalBytes: number | null;
  availableBytes: number | null;
  usedPercent: number | null;
  state: "normal" | "watch" | "critical" | "unknown";
}
type PressureKey = "cpu_load" | "cpu_core_load" | "disk_latency";
interface PressureObservation { active: boolean; high: number; low: number }
export interface ResourceObservation {
  sampledAt: number | null;
  pressures: Partial<Record<PressureKey, PressureObservation>>;
}
export interface ResourceAssessment {
  state: ResourceState;
  sampledAt: number | null;
  ageMs: number | null;
  freshness: "fresh" | "stale" | "unknown";
  quality: Record<ResourceChannel, TelemetrySampleStatus>;
  issues: ResourceIssue[];
  missing: string[];
  observation: ResourceObservation;
  cpuPercent: number | null;
  cpuPeakPercent: number | null;
  memoryPercent: number | null;
  memoryTotalBytes: number | null;
  memoryAvailableBytes: number | null;
  memoryCommitUsedBytes: number | null;
  memoryCommitLimitBytes: number | null;
  memoryCommitAvailableBytes: number | null;
  diskLatencyMs: number | null;
  volumes: ResourceVolume[];
  network: {
    receiveBps: number | null;
    transmitBps: number | null;
    receivePercent: number | null;
    transmitPercent: number | null;
    utilizationPercent: number | null;
    adapterName: string | null;
  };
}

const GIB = 1024 ** 3;
// Covers the existing 120 s background polling interval plus bounded collection time.
export const RESOURCE_STALE_AFTER_MS = 180_000;
const MIN_PRESSURE_SAMPLE_GAP_MS = 30_000;
const CHANNELS: ResourceChannel[] = ["cpu", "cpu_cores", "memory", "disk_capacity", "disk_io", "network"];

function nonnegative(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;
}
function bytes(value: unknown): number | null {
  const result = nonnegative(value);
  return result !== null && Number.isSafeInteger(result) ? result : null;
}
function percent(value: unknown): number | null {
  const result = nonnegative(value);
  return result !== null && result <= 100 ? result : null;
}
function sampleStatus(value: unknown): TelemetrySampleStatus {
  return value === "valid" || value === "warming_up" ? value : "unavailable";
}

/** Observe distinct, separated samples; rerenders and cached responses cannot confirm pressure. */
function observePressure(value: number | null, enter: number, exit: number, previous?: PressureObservation): PressureObservation {
  if (value === null) return { active: false, high: 0, low: 0 };
  const prior = previous ?? { active: false, high: 0, low: 0 };
  const high = value >= enter ? Math.min(2, prior.high + 1) : 0;
  const low = value < exit ? Math.min(2, prior.low + 1) : 0;
  return { active: prior.active ? low < 2 : high >= 2, high, low };
}

function retainPressure(value: number, enter: number, exit: number, previous?: PressureObservation): PressureObservation {
  const prior = previous ?? { active: false, high: 0, low: 0 };
  // A fast refresh cannot add confirmation, but contrary evidence must break a streak.
  return { active: prior.active, high: value >= enter ? prior.high : 0, low: value < exit ? prior.low : 0 };
}

function resolveNetwork(snapshot: SystemSnapshot, valid: boolean): ResourceAssessment["network"] {
  const result: ResourceAssessment["network"] = {
    receiveBps: valid ? bytes(snapshot.network_receive_bps) : null,
    transmitBps: valid ? bytes(snapshot.network_transmit_bps) : null,
    receivePercent: null, transmitPercent: null, utilizationPercent: null, adapterName: null
  };
  if (!valid || !Array.isArray(snapshot.network_adapters)) return result;
  // Keep the denominator and numerator on the same adapter and direction. The
  // reported link rate is not an ISP limit, and never contributes to resource state.
  for (const adapter of snapshot.network_adapters) {
    if (!adapter || adapter.rate_status !== "valid" || typeof adapter.status !== "string" || adapter.status.toLowerCase() !== "up"
      || typeof adapter.name !== "string" || !adapter.name || adapter.family_name) continue;
    const speed = bytes(adapter.link_speed_bps);
    const receive = bytes(adapter.receive_bps);
    const transmit = bytes(adapter.transmit_bps);
    if (!speed || receive === null || transmit === null) continue;
    const rx = Math.min(100, receive * 8 / speed * 100);
    const tx = Math.min(100, transmit * 8 / speed * 100);
    const utilization = Math.max(rx, tx);
    if (result.utilizationPercent === null || utilization > result.utilizationPercent) {
      Object.assign(result, { receivePercent: rx, transmitPercent: tx,
        utilizationPercent: utilization, adapterName: adapter.name });
    }
  }
  return result;
}

export function evaluateSystemResources(
  snapshot: SystemSnapshot,
  nowMs: number,
  previous?: ResourceObservation
): ResourceAssessment {
  const observed = bytes(snapshot.telemetry?.observed_at_unix_ms);
  const sampledAt = observed !== null && observed > 0 && Number.isFinite(nowMs) && observed <= nowMs + 5_000
    ? observed : null;
  const ageMs = sampledAt === null ? null : Math.max(0, nowMs - sampledAt);
  const freshness = ageMs === null ? "unknown" : ageMs >= RESOURCE_STALE_AFTER_MS ? "stale" : "fresh";
  const quality = Object.fromEntries(CHANNELS.map((key) => [key,
    freshness === "fresh" ? sampleStatus(snapshot.telemetry?.[key]) : "unavailable"
  ])) as ResourceAssessment["quality"];
  const cpuPercent = quality.cpu === "valid" ? percent(snapshot.cpu_percent) : null;
  const cpuPeakPercent = quality.cpu_cores === "valid" ? percent(snapshot.cpu_single_core_peak_percent) : null;
  const memoryTotal = quality.memory === "valid" ? bytes(snapshot.memory_total_bytes) : null;
  const memoryAvailable = quality.memory === "valid" ? bytes(snapshot.memory_available_bytes) : null;
  const memoryValid = memoryTotal !== null && memoryTotal > 0 && memoryAvailable !== null && memoryAvailable <= memoryTotal;
  const commitLimit = quality.memory === "valid" ? bytes(snapshot.memory_commit_limit_bytes) : null;
  const commitUsed = quality.memory === "valid" ? bytes(snapshot.memory_commit_used_bytes) : null;
  const commitValid = commitLimit !== null && commitLimit > 0 && commitUsed !== null && commitUsed <= commitLimit;
  const readLatency = quality.disk_io === "valid" ? nonnegative(snapshot.disk_read_latency_ms) : null;
  const writeLatency = quality.disk_io === "valid" ? nonnegative(snapshot.disk_write_latency_ms) : null;
  const diskLatencyMs = readLatency !== null && writeLatency !== null ? Math.max(readLatency, writeLatency) : null;
  const issues: ResourceIssue[] = [];
  const missing: string[] = [];
  const volumes: ResourceVolume[] = [];
  const seenVolumes = new Set<string>();
  for (const volume of Array.isArray(snapshot.disk_volumes) ? snapshot.disk_volumes : []) {
    if (!volume || typeof volume.id !== "string" || !volume.id || seenVolumes.has(volume.id)) {
      if (!missing.includes("disk_capacity")) missing.push("disk_capacity");
      continue;
    }
    seenVolumes.add(volume.id);
    const total = bytes(volume.total_bytes);
    const available = bytes(volume.available_bytes);
    const valid = freshness === "fresh" && volume.status === "valid"
      && total !== null && total > 0 && available !== null && available <= total;
    // Explicit low-space advisories, not estimates of how many games will fit.
    const severity = valid && available <= GIB ? "critical"
      : valid && (available <= 5 * GIB || (available <= 20 * GIB && available / total <= 0.05)) ? "watch" : null;
    const paths = Array.isArray(volume.paths) ? volume.paths.filter((path): path is string => typeof path === "string") : [];
    const label = displayVolumePath(volume.label) ?? paths.map(displayVolumePath).find(Boolean) ?? "";
    volumes.push({ id: volume.id, label, paths,
      totalBytes: valid ? total : null, availableBytes: valid ? available : null,
      usedPercent: valid ? (1 - available / total) * 100 : null,
      state: valid ? severity ?? "normal" : "unknown" });
    if (severity) issues.push({ code: "disk_space", severity, value: available!,
      threshold: severity === "critical" ? GIB : available! <= 5 * GIB ? 5 * GIB : 20 * GIB, target: label });
  }
  if (cpuPercent === null) missing.push("cpu");
  if (cpuPeakPercent === null) missing.push("cpu_cores");
  if (!memoryValid) missing.push("memory");
  if (!commitValid) missing.push("memory_commit");
  if (diskLatencyMs === null) missing.push("disk_io");
  if ((quality.disk_capacity !== "valid" || !volumes.length || volumes.some((volume) => volume.state === "unknown"))
    && !missing.includes("disk_capacity")) missing.push("disk_capacity");

  const pressureInputs: Array<[PressureKey, number | null, number, number]> = [
    ["cpu_load", cpuPercent, 90, 80], ["cpu_core_load", cpuPeakPercent, 95, 85], ["disk_latency", diskLatencyMs, 20, 15]
  ];
  const prior = previous?.sampledAt !== null && previous?.sampledAt !== undefined && sampledAt !== null
    && sampledAt >= previous.sampledAt && sampledAt - previous.sampledAt < RESOURCE_STALE_AFTER_MS ? previous : undefined;
  const advance = sampledAt !== null && (!prior?.sampledAt || sampledAt - prior.sampledAt >= MIN_PRESSURE_SAMPLE_GAP_MS);
  const observation: ResourceObservation = freshness !== "fresh"
    ? { sampledAt: null, pressures: {} }
    : { sampledAt: advance ? sampledAt : prior?.sampledAt ?? sampledAt, pressures: {} };
  if (freshness === "fresh") {
    for (const [code, value, enter, exit] of pressureInputs) {
      const pressure = value === null ? observePressure(null, enter, exit)
        : advance ? observePressure(value, enter, exit, prior?.pressures[code])
          : retainPressure(value, enter, exit, prior?.pressures[code]);
      observation.pressures[code] = pressure;
      if (pressure.active && value !== null) issues.push({ code, severity: "watch", value, threshold: enter,
        ...(code === "disk_latency" ? { target: displayVolumePath(snapshot.disk_label) ?? undefined } : {}) });
    }
    if (memoryValid) {
      const ratio = memoryAvailable / memoryTotal;
      const severity = memoryAvailable <= GIB && ratio <= 0.05 ? "critical"
        : memoryAvailable <= 2 * GIB && ratio <= 0.1 ? "watch" : null;
      if (severity) issues.push({ code: "memory_available", severity, value: memoryAvailable,
        threshold: severity === "critical" ? GIB : 2 * GIB });
    }
    if (commitValid) {
      const available = commitLimit - commitUsed;
      const ratio = available / commitLimit;
      const severity = available <= GIB / 2 && ratio <= 0.05 ? "critical"
        : available <= 2 * GIB && ratio <= 0.1 ? "watch" : null;
      if (severity) issues.push({ code: "memory_commit", severity, value: available,
        threshold: severity === "critical" ? GIB / 2 : 2 * GIB });
    }
  }
  issues.sort((left, right) => Number(right.severity === "critical") - Number(left.severity === "critical"));
  const state: ResourceState = freshness !== "fresh" ? freshness === "stale" ? "stale" : "unknown"
    : issues.some((issue) => issue.severity === "critical") ? "critical"
      : issues.length ? "watch" : missing.length ? "unknown" : "normal";
  return {
    state, sampledAt, ageMs, freshness, quality, issues, missing, observation,
    cpuPercent, cpuPeakPercent,
    memoryPercent: memoryValid ? (1 - memoryAvailable / memoryTotal) * 100 : null,
    memoryTotalBytes: memoryValid ? memoryTotal : null,
    memoryAvailableBytes: memoryValid ? memoryAvailable : null,
    memoryCommitUsedBytes: commitValid ? commitUsed : null,
    memoryCommitLimitBytes: commitValid ? commitLimit : null,
    memoryCommitAvailableBytes: commitValid ? commitLimit - commitUsed : null,
    diskLatencyMs, volumes, network: resolveNetwork(snapshot, quality.network === "valid")
  };
}
