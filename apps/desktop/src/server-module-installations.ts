import type { ServerModuleInstallation } from "./server-primary-action";
import type { InstanceSummary, LaunchPlan, ModuleDetails, ModuleSummary } from "./types";

export interface ServerModuleInstallationInput {
  enabled: boolean;
  modules: ModuleSummary[];
  instances: Pick<InstanceSummary, "id" | "module_id" | "status">[];
  selectedInstanceModuleDetails: ModuleDetails | null;
  selectedLaunchPlan: LaunchPlan | null;
}

export interface ServerModuleInstallationSnapshot {
  moduleInstallations: Partial<Record<string, ServerModuleInstallation>>;
  instanceLaunchPlans: Partial<Record<string, LaunchPlan>>;
  instanceLaunchFailures: Partial<Record<string, true>>;
}

interface ReadTask {
  kind: "module" | "instance";
  id: string;
  fingerprint: string;
  revision: number;
  status: "queued" | "pending" | "ready" | "failed";
  details: ModuleDetails | null;
  plan: LaunchPlan | null;
}

interface InstallationReadPorts {
  readModuleDetails: (id: string) => Promise<ModuleDetails>;
  previewInstanceLaunch: (id: string) => Promise<LaunchPlan>;
  onChange: (snapshot: ServerModuleInstallationSnapshot) => void;
  onError: (error: unknown) => void;
}

function hasManagedInstallSource(details: ModuleDetails): boolean {
  return Boolean(details.install && (details.summary.steam_app_id
    || details.install.download_url_windows || details.install.source === "minecraft_java"));
}

function sameMap<T>(left: Partial<Record<string, T>>, right: Partial<Record<string, T>>, equal: (a: T | undefined, b: T | undefined) => boolean): boolean {
  const keys = Object.keys(left);
  return keys.length === Object.keys(right).length && keys.every((key) => equal(left[key], right[key]));
}

export class ServerModuleInstallationReader {
  private tasks = new Map<string, ReadTask>();
  private modules = new Map<string, ModuleSummary>();
  private instanceModules = new Map<string, string>();
  private enabled = false;
  private epoch = 0;
  private activeReads = 0;
  private lastSelectedPlan: LaunchPlan | null = null;
  private snapshot: ServerModuleInstallationSnapshot = { moduleInstallations: {}, instanceLaunchPlans: {}, instanceLaunchFailures: {} };

  constructor(private readonly ports: InstallationReadPorts) {}

  update(input: ServerModuleInstallationInput): void {
    const reentered = input.enabled && !this.enabled;
    if (!input.enabled) this.pause();
    this.enabled = input.enabled;
    this.modules = new Map(input.modules.map((module) => [module.id, module]));
    const next = new Map<string, ReadTask>();
    const getTask = (kind: ReadTask["kind"], id: string, fingerprint: string, retainedPlan: LaunchPlan | null = null) => {
      const key = `${kind}:${id}`;
      const previous = this.tasks.get(key);
      const task: ReadTask = previous?.fingerprint === fingerprint ? previous : {
        kind, id, fingerprint, revision: 0, status: retainedPlan ? "ready" : "queued", details: null, plan: retainedPlan
      };
      if (reentered && (task.status === "failed" || (task.kind === "instance" && task.status === "ready"))) {
        task.status = "queued";
        if (!task.plan?.uses_private_runtime) task.plan = null;
        task.revision++;
      }
      next.set(key, task);
      return task;
    };

    // Check instance overrides before reading source metadata, so private installations need not wait for it.
    for (const instance of input.instances) {
      const module = this.modules.get(instance.module_id);
      const state = module?.install_state.toLowerCase() ?? "unknown";
      const status = instance.status.toLowerCase();
      const selectedPlan = input.selectedLaunchPlan !== this.lastSelectedPlan
        && input.selectedLaunchPlan?.instance_id === instance.id ? input.selectedLaunchPlan : null;
      const previousPlan = this.instanceModules.get(instance.id) === instance.module_id
        ? this.tasks.get(`instance:${instance.id}`)?.plan ?? null : null;
      const currentPlan = selectedPlan ?? previousPlan;
      const privatePlan = currentPlan?.uses_private_runtime ? currentPlan : null;
      if (state === "installed" && !privatePlan) continue;
      const active = status !== "stopped" && status !== "error";
      if (active && !privatePlan) continue;
      // Shared installation changes cannot replace an instance's known private runtime scope.
      const task = getTask("instance", instance.id,
        JSON.stringify([instance.module_id, privatePlan ? "private" : state, status, module?.version]), privatePlan);
      if (selectedPlan || (active && privatePlan)) {
        task.plan = selectedPlan ?? privatePlan;
        task.status = "ready";
        task.revision++;
      }
    }
    // Observe even previews which are not needed yet. A shared-state change must not reuse an older sample.
    this.lastSelectedPlan = input.selectedLaunchPlan;
    this.instanceModules = new Map(input.instances.map((instance) => [instance.id, instance.module_id]));

    for (const id of new Set(input.instances.map((instance) => instance.module_id))) {
      const module = this.modules.get(id);
      const task = getTask("module", id, JSON.stringify([module?.version, module?.steam_app_id]));
      const details = input.selectedInstanceModuleDetails;
      if (details?.summary.id === id && (!module || details.summary.version === module.version)
        && (!module || (details.summary.steam_app_id ?? null) === (module.steam_app_id ?? null))) {
        task.details = details;
        task.status = "ready";
        task.revision++;
      }
    }
    this.tasks = next;
    this.publish();
    this.pump();
  }

  pause(): void {
    if (!this.enabled) return;
    this.enabled = false;
    this.epoch++;
    for (const task of this.tasks.values()) {
      if (task.status === "pending") task.status = "queued";
    }
  }

  private pump(): void {
    if (!this.enabled) return;
    for (const task of this.tasks.values()) {
      if (this.activeReads >= 4) break;
      if (task.status === "queued") this.start(task);
    }
  }

  private start(task: ReadTask): void {
    const epoch = this.epoch;
    const revision = task.revision;
    const current = () => this.enabled && this.epoch === epoch && task.revision === revision
      && this.tasks.get(`${task.kind}:${task.id}`) === task;
    task.status = "pending";
    this.activeReads++;
    // Native IPC cannot be aborted. It keeps its concurrency slot until it settles, even after pause/removal.
    const read = async () => {
      if (task.kind === "module") {
        const details = await this.ports.readModuleDetails(task.id);
        if (!current()) return;
        if (details.summary.id !== task.id) throw new Error(`Module details identity does not match ${task.id}`);
        task.details = details;
      } else {
        const plan = await this.ports.previewInstanceLaunch(task.id);
        if (!current()) return;
        if (plan.instance_id !== task.id) throw new Error(`Launch preview identity does not match ${task.id}`);
        task.plan = plan;
      }
    };
    void read().then(() => {
      if (current()) task.status = "ready";
    }).catch((error: unknown) => {
      if (!current()) return;
      task.status = "failed";
      this.ports.onError(error);
    }).finally(() => {
      this.activeReads--;
      if (current()) this.publish();
      this.pump();
    });
  }

  private publish(): void {
    const next: ServerModuleInstallationSnapshot = { moduleInstallations: {}, instanceLaunchPlans: {}, instanceLaunchFailures: {} };
    for (const task of this.tasks.values()) {
      if (task.kind === "module") {
        next.moduleInstallations[task.id] = {
          installState: this.modules.get(task.id)?.install_state ?? task.details?.summary.install_state ?? "Unknown",
          hasManagedInstallSource: task.status === "failed" ? false : task.details ? hasManagedInstallSource(task.details) : null
        };
      } else {
        if (task.plan && (task.status === "ready" || task.plan.uses_private_runtime)) {
          next.instanceLaunchPlans[task.id] = task.plan;
        }
        if (task.status === "failed") next.instanceLaunchFailures[task.id] = true;
      }
    }
    const previous = this.snapshot;
    if (sameMap(previous.moduleInstallations, next.moduleInstallations, (a, b) => a?.installState === b?.installState && a?.hasManagedInstallSource === b?.hasManagedInstallSource)
      && sameMap(previous.instanceLaunchPlans, next.instanceLaunchPlans, (a, b) => a === b)
      && sameMap(previous.instanceLaunchFailures, next.instanceLaunchFailures, (a, b) => a === b)) return;
    this.snapshot = next;
    this.ports.onChange(next);
  }
}
