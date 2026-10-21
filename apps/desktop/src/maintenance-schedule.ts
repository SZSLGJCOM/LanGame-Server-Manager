import { invokeOrMock } from "./api-transport";

export type DailyMaintenanceAction = "backup" | "restart";
export interface DailyMaintenanceRun {
  local_date: string;
  status: "running" | "succeeded" | "failed" | "skipped" | "interrupted";
  started_at_unix_ms: number;
  completed_at_unix_ms: number | null;
  detail: string | null;
}
export interface DailyMaintenanceSchedule {
  instance_id: string;
  action: DailyMaintenanceAction;
  enabled: boolean;
  time_local: string;
  revision: number;
  last_run: DailyMaintenanceRun | null;
}
export interface UpdateDailyMaintenanceScheduleInput {
  instanceId: string;
  action: DailyMaintenanceAction;
  enabled: boolean;
  timeLocal: string;
  expectedRevision: number;
}
export function validDailyTime(value: string): boolean {
  return /^(?:[01]\d|2[0-3]):[0-5]\d$/.test(value);
}
export const readDailyMaintenanceSchedule = (instanceId: string, action: DailyMaintenanceAction) =>
  invokeOrMock<DailyMaintenanceSchedule>("read_instance_maintenance_schedule", { instanceId, action });
export const updateDailyMaintenanceSchedule = (input: UpdateDailyMaintenanceScheduleInput) =>
  invokeOrMock<DailyMaintenanceSchedule>("update_instance_maintenance_schedule", { input });
