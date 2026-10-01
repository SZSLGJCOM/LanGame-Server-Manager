import { resolveServerRuntimeAction, type RuntimeActionPending, type ServerRuntimeActionModel } from "./runtime-action-state";
import type { BackgroundJob, LaunchPlan } from "./types";

export interface ServerModuleInstallation {
  installState: string;
  hasManagedInstallSource: boolean | null;
}

export type ServerInstallationIntent = "install" | "repair";
export type ServerPrimaryAction = "start" | "stop" | ServerInstallationIntent | "library" | "checking";

export interface ServerPrimaryActionModel extends Omit<ServerRuntimeActionModel, "action" | "labelKey" | "iconName"> {
  action: ServerPrimaryAction;
  deleteBlocked: boolean;
  iconName: ServerRuntimeActionModel["iconName"] | "package";
  labelKey: ServerRuntimeActionModel["labelKey"] | "servers.actions.install" | "servers.actions.repair"
    | "servers.actions.openLibrary" | "servers.actions.checking" | "servers.actions.repairing"
    | "status.install.installing" | "status.install.updating" | "status.install.uninstalling";
}

export function resolveServerPrimaryAction(input: {
  instanceId: string;
  moduleId: string;
  status: string;
  hasRunningProcess?: boolean;
  pendingRuntime?: RuntimeActionPending | null;
  installation?: ServerModuleInstallation;
  launchPlan?: LaunchPlan | null;
  launchFailed?: boolean;
  pendingInstallation?: ServerInstallationIntent | null;
  jobs?: BackgroundJob[];
}): ServerPrimaryActionModel {
  const pendingRuntime = input.pendingRuntime ?? (input.status.toLowerCase() === "stopping" ? "stopping" : null);
  const runtime = resolveServerRuntimeAction(input.status, pendingRuntime, input.hasRunningProcess);
  const base = { ...runtime, deleteBlocked: runtime.action === "stop" || runtime.ariaBusy };
  if (base.deleteBlocked) return base;

  const library: ServerPrimaryActionModel = {
    ...base, action: "library", iconName: "package", labelKey: "servers.actions.openLibrary"
  };
  if (input.launchFailed) return library;
  const plan = input.launchPlan?.instance_id === input.instanceId ? input.launchPlan : null;
  const installState = String(plan?.uses_private_runtime
    ? plan.install_state
    : input.installation?.installState ?? plan?.install_state ?? "").toLowerCase();

  // Shared installation work must never replace a usable private runtime or claim
  // that downloading shared files repairs the private directory actually launched.
  if (plan?.uses_private_runtime) return installState === "installed" ? base : library;

  const activeJob = input.jobs?.find((job) => job.target_id === input.moduleId
    && ["pending", "running"].includes(job.status.toLowerCase())
    && ["downloadgame", "validategame", "uninstallgame"].includes(job.kind.toLowerCase()));
  const jobKind = activeJob?.kind.toLowerCase();
  let busyLabel: ServerPrimaryActionModel["labelKey"] | null = null;
  let busyAction: ServerPrimaryAction = "install";
  if (input.pendingInstallation === "repair" || jobKind === "validategame") {
    busyLabel = "servers.actions.repairing";
    busyAction = "repair";
  } else if (input.pendingInstallation === "install" || jobKind === "downloadgame" || installState === "installing") {
    busyLabel = "status.install.installing";
  } else if (jobKind === "uninstallgame" || installState === "uninstalling") {
    busyLabel = "status.install.uninstalling";
    busyAction = "library";
  } else if (installState === "updating") {
    busyLabel = "status.install.updating";
    busyAction = "repair";
  }
  if (busyLabel) {
    return { ...base, action: busyAction, disabled: true, ariaBusy: true, iconName: "refresh", labelKey: busyLabel,
      className: `${base.className} is-busy` };
  }
  if (installState === "installed") return base;

  if (!plan || input.installation?.hasManagedInstallSource == null) {
    return { ...base, action: "checking", disabled: true, ariaBusy: true, iconName: "refresh",
      labelKey: "servers.actions.checking", className: `${base.className} is-busy` };
  }
  if (!input.installation.hasManagedInstallSource) return library;
  if (installState === "notinstalled") {
    return { ...base, action: "install", iconName: "package", labelKey: "servers.actions.install" };
  }
  if (installState === "incomplete" || installState === "corrupted") {
    return { ...base, action: "repair", iconName: "refresh", labelKey: "servers.actions.repair" };
  }
  return library;
}

export class ServerInstallationRequestGate {
  private readonly pending = new Set<string>();

  async run(
    moduleId: string,
    intent: ServerInstallationIntent,
    install: (moduleId: string, validate: boolean) => Promise<void>
  ): Promise<boolean> {
    if (this.pending.has(moduleId)) return false;
    this.pending.add(moduleId);
    try {
      await install(moduleId, intent === "repair");
      return true;
    } finally {
      this.pending.delete(moduleId);
    }
  }
}
