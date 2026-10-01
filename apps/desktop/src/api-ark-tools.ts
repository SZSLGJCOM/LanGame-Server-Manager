import { invoke, isTauri } from "@tauri-apps/api/core";

export interface ArkToolsStatus { installed: boolean; connected: boolean; issue: string | null }
export interface ArkCreature {
  id1: number; id2: number; className: string; level: number; team: number;
  x: number; y: number; z: number; tamed: boolean;
}
export interface ArkSpawnInput {
  instanceId: string; requestId: string; creature: string; level: number;
  x: number; y: number; z: number; tamed: boolean; playerId: number;
}
export interface ArkSpawnResult { instanceId: string; requestId: string; creature: ArkCreature }

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
  return { installed: response.installed, connected: response.connected, issue: response.issue as string | null };
}
export async function readArkToolsStatus(instanceId: string): Promise<ArkToolsStatus> {
  requireHost();
  return status(await invoke<unknown>("read_ark_tools_status", { input: { instanceId } }));
}
export async function prepareArkTools(instanceId: string): Promise<ArkToolsStatus> {
  requireHost();
  return status(await invoke<unknown>("prepare_ark_tools", { input: { instanceId, allowMatchingSymbolsDownload: true } }));
}
export async function spawnArkCreature(input: ArkSpawnInput): Promise<ArkSpawnResult> {
  requireHost();
  const value = record(await invoke<unknown>("spawn_ark_creature", { input }));
  const entity = record(value.creature);
  const integer = (key: string, max: number) => typeof entity[key] === "number" && Number.isSafeInteger(entity[key]) && entity[key] >= 0 && entity[key] <= max;
  if (value.instanceId !== input.instanceId || value.requestId !== input.requestId
    || !integer("id1", 0xffffffff) || !integer("id2", 0xffffffff) || entity.id1 === 0 && entity.id2 === 0
    || !integer("level", 20000) || entity.level === 0
    || typeof entity.team !== "number" || !Number.isSafeInteger(entity.team)
    || typeof entity.className !== "string" || entity.className.length > 300
    || typeof entity.tamed !== "boolean" || entity.tamed !== input.tamed
    || [entity.x, entity.y, entity.z].some(v => typeof v !== "number" || !Number.isFinite(v))) {
    throw new Error("Invalid ARK creature read-back. Check the server before generating another creature.");
  }
  return { instanceId: input.instanceId, requestId: input.requestId, creature: {
    id1: entity.id1 as number, id2: entity.id2 as number, className: entity.className,
    level: entity.level as number, team: entity.team, x: entity.x as number,
    y: entity.y as number, z: entity.z as number, tamed: entity.tamed
  } };
}
