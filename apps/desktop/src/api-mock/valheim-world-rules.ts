import type { ValheimWorldRules } from "../valheim-world";

export function buildMockValheimWorldRules(instanceId: string, worldName: string): ValheimWorldRules {
  return { instance_id: instanceId, world_name: worldName, source: "saved", world_version: 41, saved_keys: [] };
}
