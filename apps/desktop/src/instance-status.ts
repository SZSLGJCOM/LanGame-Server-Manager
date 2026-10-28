export function normalizeInstanceStatus(status: string | null | undefined): string {
  const value = String(status ?? "").trim().toLowerCase();
  if (!value) return "unknown";
  if (value.includes("run")) return "running";
  if (value.includes("stopping")) return "stopping";
  if (value.includes("stop")) return "stopped";
  if (value.includes("start")) return "starting";
  if (value.includes("error") || value.includes("fail") || value.includes("crash")) return "error";
  return value;
}
