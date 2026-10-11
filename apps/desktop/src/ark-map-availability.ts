import { canonicalAsaModId } from "./views/servers/mod-workbench-asa";

export interface InstalledArkMap {
  map_name: string;
  mod_id: string;
  label: string;
}

export interface InstanceArkMapAvailability {
  instance_id: string;
  module_id: "arksurvivalascended";
  maps: InstalledArkMap[];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isInstalledArkMap(value: unknown): value is InstalledArkMap {
  return isRecord(value) && typeof value.map_name === "string" && /^[A-Za-z0-9_]{1,128}$/.test(value.map_name)
    && typeof value.mod_id === "string" && canonicalAsaModId(value.mod_id) === value.mod_id
    && typeof value.label === "string" && value.label.trim().length > 0 && [...value.label].length <= 256;
}

/** The response contains native package identifiers discovered in this instance, never inferred from titles. */
export function parseInstanceArkMapAvailability(value: unknown, instanceId: string): InstanceArkMapAvailability {
  if (!isRecord(value) || value.instance_id !== instanceId || value.module_id !== "arksurvivalascended"
    || !Array.isArray(value.maps)) {
    throw new Error("Invalid installed ARK map availability response.");
  }
  const maps: unknown[] = value.maps;
  if (maps.length > 8192 || !maps.every(isInstalledArkMap)) throw new Error("Invalid installed ARK map availability response.");
  return { instance_id: instanceId, module_id: "arksurvivalascended",
    maps: maps.map((map) => ({ map_name: map.map_name, mod_id: map.mod_id, label: map.label })) };
}
