/** Quality belongs to the observation, not to whether an RPC returned successfully. */
export type TelemetrySampleStatus = "valid" | "warming_up" | "unavailable";

export interface SystemTelemetry {
  observed_at_unix_ms: number | null;
  cpu: TelemetrySampleStatus;
  cpu_cores: TelemetrySampleStatus;
  memory: TelemetrySampleStatus;
  disk_capacity: TelemetrySampleStatus;
  disk_io: TelemetrySampleStatus;
  network: TelemetrySampleStatus;
}

export interface SystemDiskVolume {
  id: string;
  label: string;
  paths: string[];
  total_bytes: number;
  available_bytes: number;
  free_bytes: number;
  status: TelemetrySampleStatus;
}
