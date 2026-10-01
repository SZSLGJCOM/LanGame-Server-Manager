import type { InstanceSummary, PortBinding } from "./types";

export interface ArkClusterIdentity {
  module_id: string;
  cluster_id: string;
  directory_key: string;
  member_ids: string[];
}

export interface ArkClusterMember {
  summary: InstanceSummary;
  map_name: string;
  cluster_id: string;
  cluster_directory: string | null;
  explicit_shared_directory: boolean;
  config_file_path: string;
  saves_path: string;
  ports: PortBinding[];
}

export interface ArkClusterIssue {
  code: string;
  severity: "error" | "warning";
  instance_id: string;
  instance_name: string;
  message: string;
  path: string | null;
}

export interface ArkClusterReport {
  instance_id: string;
  identity: ArkClusterIdentity | null;
  cluster_directory: string | null;
  members: ArkClusterMember[];
  related_instances: ArkClusterMember[];
  issues: ArkClusterIssue[];
  start_blocked: boolean;
}

export type ArkClusterAction = "start" | "stop";
export interface OperateArkClusterInput {
  instance_id: string;
  expected_identity: ArkClusterIdentity;
  action: ArkClusterAction;
}

export interface ArkClusterOperationResult {
  action: ArkClusterAction;
  members: Array<{
    instance_id: string;
    instance_name: string;
    outcome: "succeeded" | "skipped" | "failed";
    message: string;
  }>;
}

export function isArkModule(moduleId: string): boolean {
  return moduleId === "arksurvivalevolved" || moduleId === "arksurvivalascended";
}

export function clusterOperationCounts(result: ArkClusterOperationResult) {
  return result.members.reduce((counts, member) => {
    counts[member.outcome] += 1;
    return counts;
  }, { succeeded: 0, skipped: 0, failed: 0 });
}
