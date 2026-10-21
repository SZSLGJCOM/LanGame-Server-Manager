import { measureControl, type ControlMeasurement } from "./measure-control-contracts";

type Section = "saves" | "backups" | "runtime" | "broadcast";

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
  measure("backup action", ".server-backup-history-card .maintenance-toolbar > button.secondary-button", 30, "action");
  await click('[data-maintenance-section="backups"] .server-file-backup-actions > button:nth-of-type(2)');
  measure("backup rename input", ".backup-rename-action > input", 38);
  measure("backup rename save", ".backup-rename-action > button[type=submit]", 38, "action");
  measure("backup rename cancel", ".backup-rename-action > button[type=button]", 38, "action");
  await click(".backup-rename-action > button[type=button]");
  await navigate("backups");
  measure("save retention", ".server-managed-backup-policy input[type=number]", 38);
  measure("save policy toggle", ".server-backup-policy-toggle", 38);
  await navigate("runtime");
  // Runtime policies live in chip popovers; open one at a time, as a user would.
  for (const [chip, name, selector] of [
    ["server-resource-policy", "runtime CPU limit", ".server-resource-fields input[name=cpu]"],
    ["server-runtime-recovery-chip", "runtime recovery attempts", ".server-runtime-recovery-editor input[name=maxRestarts]"]
  ] as const) {
    const summary = `.${chip} > .maintenance-chip__summary`;
    if (fixture.querySelector(summary)?.getAttribute("aria-expanded") !== "true") await click(summary);
    measure(name, selector, 38);
    await click(`.${chip} > .maintenance-chip__summary`);
  }
  measure("storage refresh action", ".instance-isolation-refresh", 30, "action");
  await navigate("broadcast");
  measure("broadcast tone", "select[name=broadcast-tone]", 38);
  measure("broadcast cooldown", "input[name=broadcast-cooldown]", 38);
  measure("broadcast generate action", ".server-broadcast-actions .secondary-button", 38, "action");
  measure("broadcast send action", ".server-broadcast-actions .primary-button", 38, "action");
  measure("broadcast refresh action", ".server-broadcast-history-heading > button", 28, "action");
  const checkbox = fixture.querySelector<HTMLInputElement>("input[name=broadcast-enabled]")!;
  const style = getComputedStyle(checkbox);
  if (checkbox.getBoundingClientRect().width !== 18 || checkbox.getBoundingClientRect().height !== 18
    || style.appearance !== "none") throw new Error("Broadcast checkbox does not share the 18px configuration appearance");
  await navigate("backups");
  return measurements;
}
