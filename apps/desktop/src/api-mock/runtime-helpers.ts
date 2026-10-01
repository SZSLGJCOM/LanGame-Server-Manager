import type { InstanceDetails } from "../types";

export function mockRuntimeTransportIsRemote(transport: string): boolean {
  return ["source_rcon", "humanitz_rcon", "palworld_rest", "websocket_rcon", "battleye_rcon", "telnet"]
    .includes(transport);
}

export function mockRuntimeTransportLabel(transport: string): string {
  if (transport === "source_rcon") return "rcon";
  if (transport === "humanitz_rcon") return "humanitz-rcon";
  if (transport === "palworld_rest") return "rest-api";
  if (transport === "websocket_rcon") return "web-rcon";
  if (transport === "battleye_rcon") return "be-rcon";
  if (transport === "telnet") return "telnet";
  return "console";
}

export function mockRuntimeDisplayName(transport: string, processKey: string): string {
  if (transport === "source_rcon") return "RCON";
  if (transport === "humanitz_rcon") return "RCON";
  if (transport === "palworld_rest") return "Palworld REST API";
  if (transport === "websocket_rcon") return "WebSocket RCON";
  if (transport === "battleye_rcon") return "BattlEye RCON";
  if (transport === "telnet") return "Telnet";
  return processKey === "caves" ? "Caves" : "Master";
}

export function withMockProcessHostSurface<T extends object>(
  process: T,
  hostSurface: string
): T & { host_surface: string } {
  return {
    ...process,
    host_surface: hostSurface
  };
}

export function mockInstanceRootFromDetails(details: InstanceDetails): string {
  const normalized = details.config_file_path.replace(/\\/g, "/");
  const configSuffix = "/config/instance.json";
  if (normalized.endsWith(configSuffix)) {
    return normalized.slice(0, -configSuffix.length);
  }
  return `D:/LanGame/instances/${details.summary.id}`;
}

export function mockPathIsWithinRoot(path: string, root: string): boolean {
  const normalizedPath = path.replace(/\\/g, "/");
  const normalizedRoot = root.replace(/\\/g, "/").replace(/\/+$/g, "");
  return normalizedPath === normalizedRoot || normalizedPath.startsWith(`${normalizedRoot}/`);
}

export function mockInferManualModId(idStrategy: string | null | undefined, name: string): string | null {
  if (idStrategy !== "numeric_prefix") {
    return null;
  }
  const match = name.match(/^(\d{5,})(?:_|$)/);
  return match?.[1] ?? null;
}
