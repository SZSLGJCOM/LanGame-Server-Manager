import { invokeOrMock } from "./api-transport";

export interface AstroneerSaveEntry {
  descriptive_name: string;
  latest_saved_at: string;
  versions: number;
  total_bytes: number;
}

export interface AstroneerSaveCatalog {
  instance_id: string;
  configured_name: string;
  entries: AstroneerSaveEntry[];
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function parseAstroneerSaveCatalog(value: unknown, instanceId: string): AstroneerSaveCatalog {
  if (!record(value) || value.instance_id !== instanceId || typeof value.configured_name !== "string" ||
    !Array.isArray(value.entries) || value.entries.length > 256) {
    throw new Error("Invalid ASTRONEER save catalog response.");
  }
  const names = new Set<string>();
  const entries = value.entries.map((entry): AstroneerSaveEntry => {
    if (!record(entry) || typeof entry.descriptive_name !== "string" || !entry.descriptive_name.trim() ||
      typeof entry.latest_saved_at !== "string" || typeof entry.versions !== "number" ||
      !Number.isSafeInteger(entry.versions) || entry.versions < 1 || typeof entry.total_bytes !== "number" ||
      !Number.isSafeInteger(entry.total_bytes) || entry.total_bytes < 1 || names.has(entry.descriptive_name)) {
      throw new Error("Invalid ASTRONEER save catalog entry.");
    }
    names.add(entry.descriptive_name);
    return { descriptive_name: entry.descriptive_name, latest_saved_at: entry.latest_saved_at,
      versions: entry.versions, total_bytes: entry.total_bytes };
  });
  return { instance_id: instanceId, configured_name: value.configured_name, entries };
}

export async function readAstroneerSaveCatalog(instanceId: string): Promise<AstroneerSaveCatalog> {
  return parseAstroneerSaveCatalog(
    await invokeOrMock<unknown>("read_astroneer_save_catalog", { instanceId }), instanceId
  );
}
