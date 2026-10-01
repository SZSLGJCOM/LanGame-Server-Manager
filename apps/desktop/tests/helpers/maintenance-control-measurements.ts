import { measureControl, type ControlMeasurement } from "./measure-control-contracts";

type Section = "backups" | "save-policy" | "runtime" | "storage" | "broadcast";

export async function measureMaintenanceControls(
  fixture: HTMLElement,
  navigate: (section: Section) => Promise<void>,
  click: (selector: string) => Promise<void>
): Promise<ControlMeasurement[]> {
  const measurements: ControlMeasurement[] = [];
  function measure(name: string, selector: string, height: number, role: "field" | "action" = "field") {
    const element = fixture.querySelector<HTMLElement>(selector);
    if (!element || element.getClientRects().length === 0) throw new Error(`Missing visible control: ${name}`);
    measurements.push(measureControl(name, element, height, role));
  }
  await navigate("backups");
  measure("backup action", ".server-file-backup-toolbar button", 30, "action");
  await click(".server-file-backup-actions > button:nth-of-type(2)");
  measure("backup rename input", ".backup-rename-action > input", 38);
  measure("backup rename save", ".backup-rename-action > button[type=submit]", 38, "action");
  measure("backup rename cancel", ".backup-rename-action > button[type=button]", 38, "action");
  await click(".backup-rename-action > button[type=button]");
  await navigate("save-policy");
  measure("save retention", ".server-managed-backup-policy input[type=number]", 38);
  measure("save policy toggle", ".server-backup-policy-toggle", 38);
  measure("save policy action", ".server-backup-policy-card button[type=submit]", 30, "action");
  await navigate("runtime");
  measure("runtime CPU limit", ".server-resource-fields input[name=cpu]", 38);
  measure("runtime autostart toggle", ".server-autostart-policy-editor > .settings-toggle-card", 38);
  measure("runtime recovery attempts", ".server-runtime-recovery-editor input[name=maxRestarts]", 38);
  measure("runtime recovery action", ".server-runtime-recovery-editor button[type=submit]", 30, "action");
  await navigate("storage");
  measure("storage refresh action", ".instance-isolation-refresh", 30, "action");
  await navigate("broadcast");
  measure("broadcast tone", "select[name=broadcast-tone]", 38);
  measure("broadcast cooldown", "input[name=broadcast-cooldown]", 38);
  measure("broadcast generate action", ".server-broadcast-actions > .secondary-button", 38, "action");
  measure("broadcast send action", ".server-broadcast-actions > .primary-button", 38, "action");
  measure("broadcast refresh action", ".server-broadcast-history-heading > button", 28, "action");
  const checkbox = fixture.querySelector<HTMLInputElement>("input[name=broadcast-enabled]")!;
  const style = getComputedStyle(checkbox);
  if (checkbox.getBoundingClientRect().width !== 18 || checkbox.getBoundingClientRect().height !== 18
    || style.appearance !== "none") throw new Error("Broadcast checkbox does not share the 18px configuration appearance");
  await navigate("backups");
  return measurements;
}
