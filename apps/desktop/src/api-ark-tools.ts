import { invoke, isTauri } from "@tauri-apps/api/core";
import { parseArkMap, validArkMapScope, type ArkMapScope } from "./ark-world-map";
import type { InstanceRuntimeCommandResult } from "./types";

export interface ArkToolsStatus { installed: boolean; connected: boolean; issue: string | null }
export interface ArkCreature {
  id1: number; id2: number; className: string; level: number; team: number;
  x: number; y: number; z: number; tamed: boolean;
}
export interface ArkSpawnInput extends ArkMapScope {
  instanceId: string; requestId: string; creature: string; level: number;
  x: number; y: number; z: number; tamed: boolean; playerId: number;
}
export interface ArkSpawnResult extends ArkMapScope { requestId: string; creature: ArkCreature }

export function arkToolsHostAvailable(): boolean { return isTauri(); }
function requireHost() {
  if (!arkToolsHostAvailable()) throw new Error("ARK creature tools require the local desktop host.");
}
function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid ARK tool response.");
  return value as Record<string, unknown>;
}
function status(value: unknown): ArkToolsStatus {
  const response = record(value);
  if (typeof response.installed !== "boolean" || typeof response.connected !== "boolean"
    || response.issue !== null && (typeof response.issue !== "string" || response.issue.length > 8192)
    || response.connected && !response.installed) throw new Error("Invalid ARK extension status.");
  // The native installer reports ordinary absence as a message. Present that
  // prerequisite through localized action help; retain actual repair/connection diagnostics.
  const issue = !response.installed && response.issue === "Prepare the ARK server extension while this instance is stopped"
    ? null : response.issue as string | null;
  return { installed: response.installed, connected: response.connected, issue };
}
export async function readArkToolsStatus(instanceId: string, processKey = "main"): Promise<ArkToolsStatus> {
  requireHost();
  return status(await invoke<unknown>("read_ark_tools_status", { input: { instanceId, processKey } }));
}
export async function prepareArkTools(instanceId: string): Promise<ArkToolsStatus> {
  requireHost();
  return status(await invoke<unknown>("prepare_ark_tools", { input: { instanceId, allowMatchingSymbolsDownload: true } }));
}
export async function spawnArkCreature(input: ArkSpawnInput): Promise<ArkSpawnResult> {
  requireHost();
  if (!validArkMapScope(input)) throw new Error("Select a running ARK map before spawning.");
  const value = record(await invoke<unknown>("spawn_ark_creature", { input }));
  const entity = record(value.creature);
  const integer = (key: string, max: number) => typeof entity[key] === "number" && Number.isSafeInteger(entity[key]) && entity[key] >= 0 && entity[key] <= max;
  if (value.instanceId !== input.instanceId || value.processKey !== input.processKey || value.runId !== input.runId || value.requestId !== input.requestId
    || !integer("id1", 0xffffffff) || !integer("id2", 0xffffffff) || entity.id1 === 0 && entity.id2 === 0
    || !integer("level", 20000) || entity.level === 0
    || typeof entity.team !== "number" || !Number.isSafeInteger(entity.team)
    || typeof entity.className !== "string" || entity.className.length > 300
    || typeof entity.tamed !== "boolean" || entity.tamed !== input.tamed
    || [entity.x, entity.y, entity.z].some(v => typeof v !== "number" || !Number.isFinite(v))) {
    throw new Error("Invalid ARK creature read-back. Check the server before generating another creature.");
  }
  return { instanceId: input.instanceId, processKey: input.processKey, runId: input.runId, requestId: input.requestId, creature: {
    id1: entity.id1 as number, id2: entity.id2 as number, className: entity.className,
    level: entity.level as number, team: entity.team, x: entity.x as number,
    y: entity.y as number, z: entity.z as number, tamed: entity.tamed
  } };
}

export async function readArkMap(scope: ArkMapScope) {
  requireHost();
  if (!validArkMapScope(scope)) throw new Error("Select a running ARK map first.");
  return parseArkMap(await invoke<unknown>("read_ark_map", { input: scope }), scope);
}
export async function sendArkMapGmCommand(scope: ArkMapScope, command: string): Promise<InstanceRuntimeCommandResult> {
  requireHost();
  if (!validArkMapScope(scope)) throw new Error("Select a running ARK map first.");
  const value = record(await invoke<unknown>("send_ark_map_gm_command", { input: { ...scope, command } }));
  if (value.instance_id !== scope.instanceId || value.process_key !== scope.processKey || value.command !== command
    || value.response_text != null && (typeof value.response_text !== "string" || value.response_text.length > 1048576)
    || typeof value.write_confirmation_pending !== "boolean" || typeof value.display_name !== "string"
    || !Number.isSafeInteger(value.pid) || !Number.isSafeInteger(value.submitted_at_unix_ms)) throw new Error("Invalid ARK command receipt. Check the server before retrying.");
  return value as unknown as InstanceRuntimeCommandResult;
}
