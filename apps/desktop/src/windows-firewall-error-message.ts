import type { TranslateFn } from "./i18n";

const reasons = new Set(["invalid_rules", "rule_conflict", "permission_denied", "elevation_cancelled", "verification_failed", "cleanup_failed", "timeout", "apply_failed"]);

export function formatWindowsFirewallError(t: TranslateFn, value: Record<string, unknown>): string | null {
  if (value.code !== "windows_firewall_setup_failed" || typeof value.reason !== "string" || !reasons.has(value.reason)) return null;
  if (["instance_name", "local_address", "log_path"].some((key) => value[key] !== undefined && value[key] !== null && typeof value[key] !== "string")) return null;
  if (value.continue_start !== undefined && typeof value.continue_start !== "boolean") return null;
  const lines = [t("errors.firewallSetupFailed", { reason: t(`errors.firewallReason.${value.reason}`) })];
  if (typeof value.instance_name === "string") lines.push(t("errors.firewallInstance", { instance: value.instance_name }));
  if (value.continue_start === false) lines.push(t("errors.firewallStrictStartBlocked", { address: typeof value.local_address === "string" ? value.local_address : "" }));
  if (value.continue_start === true) lines.push(t("errors.firewallStartContinued"));
  lines.push(t("errors.firewallSetupAdvice"));
  if (typeof value.log_path === "string") lines.push(t("errors.firewallLog", { path: value.log_path }));
  return lines.join("\n");
}
