import type {
  InstanceDetails,
  InstanceIsolationConflict,
  InstanceIsolationReport
} from "../types";
import { mockInstanceRootFromDetails } from "./runtime-helpers";

export const mockInstancePrograms = new Map<string, { mode: "shared" | "independent"; root: string }>();

function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

function overlaps(left: string, right: string): boolean {
  const a = normalizePath(left);
  const b = normalizePath(right);
  return Boolean(a && b && (a === b || a.startsWith(b + "/") || b.startsWith(a + "/")));
}

function runtimePath(details: InstanceDetails): string {
  return mockInstancePrograms.get(details.summary.id)?.root ?? mockInstanceRootFromDetails(details) + "/runtime";
}

function configPath(details: InstanceDetails): string {
  return details.config_file_path.replace(/[\\/][^\\/]+$/, "");
}

export function buildMockInstanceIsolation(details: InstanceDetails, instances: InstanceDetails[]): InstanceIsolationReport {
  const paths = {
    configuration: configPath(details),
    runtime: runtimePath(details),
    saves: details.saves_path
  };
  const conflicts: InstanceIsolationConflict[] = [];
  for (const other of instances) {
    if (other.summary.id === details.summary.id) continue;
    const otherPaths = {
      configuration: configPath(other),
      runtime: runtimePath(other),
      saves: other.saves_path
    };
    for (const kind of ["configuration", "runtime", "saves"] as const) {
      if (kind === "runtime" && mockInstancePrograms.get(details.summary.id)?.mode === "shared"
        && mockInstancePrograms.get(other.summary.id)?.mode === "shared"
        && normalizePath(paths.runtime) === normalizePath(otherPaths.runtime)) continue;
      if (overlaps(paths[kind], otherPaths[kind])) {
        conflicts.push({
          instance_id: other.summary.id,
          instance_name: other.summary.name,
          kind,
          path: paths[kind],
          other_path: otherPaths[kind]
        });
      }
    }
  }
  return {
    instance_id: details.summary.id,
    mode: mockInstancePrograms.get(details.summary.id)?.mode === "shared" ? "shared" : "private",
    runtime_path: paths.runtime,
    data_path: mockInstanceRootFromDetails(details),
    config_path: paths.configuration,
    saves_path: paths.saves,
    conflicts,
    issues: []
  };
}
