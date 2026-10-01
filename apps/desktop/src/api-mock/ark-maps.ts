import { isArkModule } from "../ark-clusters";
import type { InstanceDetails, InstanceProcessRecord, InstanceSummary, LogTailSnapshot, PortBinding } from "../types";
import { readAdditionalArkMaps, validateAdditionalArkMaps } from "../views/settings/ark-cluster-maps";
import type { SettingsObject } from "../views/settings/settings-schema";

function validatedMaps(settings: SettingsObject) {
  const reason = validateAdditionalArkMaps(settings);
  if (reason) throw new Error(`Invalid ARK additional_maps: ${reason}`);
  return readAdditionalArkMaps(settings)!;
}

export function buildMockArkProcesses(summary: Pick<InstanceSummary, "id" | "module_id">, settings: SettingsObject): InstanceProcessRecord[] {
  const mapName = settings.map_name ?? (summary.module_id === "arksurvivalascended" ? "TheIsland_WP" : "TheIsland");
  if (typeof mapName !== "string" || !/^[A-Za-z0-9_]{1,128}$/.test(mapName)) throw new Error("Invalid ARK main map package.");
  const names = [{ key: "main", name: mapName }, ...validatedMaps(settings).filter((map) => map.enabled).map((map) => ({ key: `map-${map.id}`, name: map.name }))];
  return names.map((map, index) => ({ run_id: index + 1, session_id: "demo-session", process_key: map.key,
    display_name: map.name, pid: 4321 + index, status: "running", is_primary: index === 0,
    log_path: `D:/LanGame/instances/${summary.id}/logs/run-1-${map.key}.log` }));
}

export function updateMockArkPorts(current: InstanceDetails, settings: SettingsObject, defaults: PortBinding[], incoming: PortBinding[]): PortBinding[] {
  const previous = validatedMaps(JSON.parse(current.settings_json) as SettingsObject);
  const maps = validatedMaps(settings);
  if ((current.active_run || ["starting", "running", "stopping"].includes(current.summary.status.toLowerCase()))
    && JSON.stringify(previous) !== JSON.stringify(maps)) throw new Error("Stop the ARK instance before changing its map servers.");
  for (const map of maps) {
    if (previous.some((old) => old.id === map.id && old.map_name !== map.map_name)) throw new Error("An existing map's package cannot change. Add a new map to retain its world.");
  }
  const ports = incoming.filter((port) => !port.name.startsWith("map-")).map((port) => ({ ...port }));
  const used = new Set(ports.map((port) => `${port.protocol}:${port.port}`));
  // Reserve every saved map before a new map can claim one of its endpoints.
  const retained = maps.map((map) => ({ map, bindings: defaults.flatMap((defaultPort) => {
    const name = `map-${map.id}-${defaultPort.name}`;
    const port = incoming.find((port) => port.name === name) ?? current.ports.find((port) => port.name === name);
    if (!port) return [];
    if (port.protocol !== defaultPort.protocol || !Number.isInteger(port.port) || port.port <= 0 || port.port > 65535) throw new Error(`ARK map binding ${name} must have a nonzero ${defaultPort.protocol} port.`);
    used.add(`${port.protocol}:${port.port}`);
    return [{ ...port }];
  }) }));
  for (const { map, bindings } of retained) {
    for (const defaultPort of defaults) {
      const name = `map-${map.id}-${defaultPort.name}`;
      if (bindings.some((port) => port.name === name)) continue;
      let candidate = defaultPort.port;
      const needsPeer = current.summary.module_id === "arksurvivalevolved" && defaultPort.name === "game";
      while (used.has(`${defaultPort.protocol}:${candidate}`) || (needsPeer && used.has(`${defaultPort.protocol}:${candidate + 1}`))) candidate += 10;
      if (current.summary.module_id === "arksurvivalevolved" && defaultPort.name === "peer") {
        const game = bindings.find((port) => port.name === `map-${map.id}-game`);
        if (game) candidate = game.port + 1;
      }
      if (candidate < 1 || candidate > 65535 || (needsPeer && candidate === 65535)) throw new Error("No port remains for the additional ARK map.");
      used.add(`${defaultPort.protocol}:${candidate}`);
      bindings.push({ name, protocol: defaultPort.protocol, port: candidate });
    }
    ports.push(...bindings);
  }
  return ports;
}

export function mockArkCommandTarget(details: InstanceDetails, processKey: string, transport: string): InstanceProcessRecord | null {
  if (!isArkModule(details.summary.module_id)) return null;
  const process = details.active_run?.processes?.find((process) => process.process_key === processKey && process.status?.toLowerCase() === "running" && process.pid);
  if (!process) throw new Error(`ARK map process ${processKey} is not running.`);
  if (transport !== "source_rcon") throw new Error("ARK commands require Source RCON.");
  const settings = JSON.parse(details.settings_json) as SettingsObject;
  if (settings.rcon_enabled !== true) throw new Error("Enable RCON before sending ARK commands.");
  if (typeof settings.admin_password !== "string" || !settings.admin_password.trim()) throw new Error("Configure the ARK administrator password before sending commands.");
  const name = processKey === "main" ? "rcon" : `${processKey}-rcon`;
  if (!details.ports.some((port) => port.name === name && port.protocol === "tcp" && port.port > 0)) throw new Error(`ARK map process ${processKey} has no RCON endpoint.`);
  return process;
}

/** Development preview log boundary: each map owns a separate bounded stream. */
export class MockArkMapLogs {
  private readonly documents = new Map<string, LogTailSnapshot>();

  clear(instanceId: string) {
    for (const key of this.documents.keys()) if (key.startsWith(`${instanceId}:`)) this.documents.delete(key);
  }

  read(details: InstanceDetails, process: InstanceProcessRecord, maxLines: number): LogTailSnapshot {
    const key = `${details.summary.id}:${process.process_key}`;
    let log = this.documents.get(key);
    if (!log) {
      const portName = `${process.process_key}-game`;
      log = { source_path: process.log_path, lines: [`[info] ${process.display_name} dedicated map started`,
        `[info] listening on ${details.summary.bind_ip}:${details.ports.find((port) => port.name === portName)?.port ?? 0}`], total_lines: 2, truncated: false, read_error: null };
      this.documents.set(key, log);
    }
    const limit = Math.max(1, Math.min(2000, maxLines || 200));
    return { ...log, lines: log.lines.slice(-limit), truncated: log.lines.length > limit };
  }

  append(details: InstanceDetails, process: InstanceProcessRecord, line: string) {
    const snapshot = this.read(details, process, 2000);
    this.documents.set(`${details.summary.id}:${process.process_key}`, { ...snapshot,
      lines: [...snapshot.lines, line].slice(-2000), total_lines: (snapshot.total_lines ?? snapshot.lines.length) + 1 });
  }
}
