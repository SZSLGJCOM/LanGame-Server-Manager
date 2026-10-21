import { validDailyTime, type DailyMaintenanceAction, type DailyMaintenanceSchedule,
  type UpdateDailyMaintenanceScheduleInput } from "./maintenance-schedule";

const policies = new Map<string, DailyMaintenanceSchedule>();
const key = (instanceId: string, action: DailyMaintenanceAction) => `${instanceId}:${action}`;

export function readMockDailyMaintenance(instanceId: string, action: DailyMaintenanceAction): DailyMaintenanceSchedule {
  return structuredClone(policies.get(key(instanceId, action)) ?? { instance_id: instanceId, action,
    enabled: false, time_local: "04:00", revision: 0, last_run: null });
}

export function updateMockDailyMaintenance(input: UpdateDailyMaintenanceScheduleInput): DailyMaintenanceSchedule {
  if (!validDailyTime(input.timeLocal)) throw new Error("Daily maintenance time must use HH:mm (00:00–23:59).");
  const current = readMockDailyMaintenance(input.instanceId, input.action);
  if (input.expectedRevision !== current.revision) throw new Error("Daily maintenance policy changed elsewhere. Reload it before saving.");
  const saved = { ...current, enabled: input.enabled, time_local: input.timeLocal, revision: current.revision + 1 };
  policies.set(key(input.instanceId, input.action), saved);
  return structuredClone(saved);
}
