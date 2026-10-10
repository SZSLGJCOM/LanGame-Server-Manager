import { invokeOrMock } from "./api-transport";

export interface ValheimWorldRules {
  instance_id: string;
  world_name: string;
  source: "saved" | "new_world" | "missing_metadata";
  world_version: number | null;
  saved_keys: string[];
}

export function parseValheimWorldRules(value: unknown, instanceId: string, worldName: string): ValheimWorldRules {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid Valheim world rules response.");
  const record = value as Record<string, unknown>;
  if (record.instance_id !== instanceId || record.world_name !== worldName ||
    !["saved", "new_world", "missing_metadata"].includes(String(record.source)) ||
    !(record.world_version === null || typeof record.world_version === "number" &&
      Number.isSafeInteger(record.world_version) && record.world_version >= 26 && record.world_version <= 41) ||
    !Array.isArray(record.saved_keys) || record.saved_keys.length > 512 ||
    !record.saved_keys.every((key): key is string => typeof key === "string" && key.length <= 1024 && !/[\u0000-\u001f\u007f]/.test(key)) ||
    (record.source === "saved" ? record.world_version === null : record.world_version !== null || record.saved_keys.length !== 0)) {
    throw new Error("Invalid Valheim world rules response.");
  }
  return record as unknown as ValheimWorldRules;
}

export async function readValheimWorldRules(instanceId: string, worldName: string): Promise<ValheimWorldRules> {
  return parseValheimWorldRules(await invokeOrMock<unknown>("read_valheim_world_rules", { instanceId, worldName }),
    instanceId, worldName);
}
