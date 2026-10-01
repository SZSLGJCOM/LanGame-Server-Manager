import type { TranslateFn } from "../../i18n";
import type { InstanceRemovalPlan } from "../../storage-management-types";

export function formatInstanceRemovalPlan(plan: InstanceRemovalPlan, t: TranslateFn): string {
  const removed = [...new Set([plan.data_path, ...plan.owned_data_paths,
    ...(plan.remove_program ? [plan.program_path] : [])].filter(Boolean))];
  const preserved = [plan.preserved_external_saves_path].filter(Boolean);
  return [t("servers.removal.confirm"), t("servers.removal.removed", { paths: removed.join("\n") }),
    ...(preserved.length ? [t("servers.removal.preserved", { paths: preserved.join("\n") })] : []),
    ...(plan.preserved_program_path ? [t("servers.removal.libraryProgram", { path: plan.preserved_program_path })] : []),
    t("servers.removal.cleanupHint")].join("\n\n");
}
