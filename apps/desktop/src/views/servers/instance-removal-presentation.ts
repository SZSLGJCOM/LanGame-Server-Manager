import type { TranslateFn } from "../../i18n";
import type { InstanceRemovalPlan } from "../../storage-management-types";

export function formatInstanceRemovalPlan(plan: InstanceRemovalPlan, t: TranslateFn): string {
  return [t("servers.removal.confirm"), t(plan.remove_program ? "servers.removal.programRemoved" : "servers.removal.cleanupHint"),
    ...(plan.preserved_external_saves_path ? [t("servers.removal.preserved")] : [])].join("\n");
}
