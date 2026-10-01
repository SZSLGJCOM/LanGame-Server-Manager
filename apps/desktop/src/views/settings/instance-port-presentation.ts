import type {
  InstanceDetails,
  ModulePortGroupDetails,
  ModulePortRole,
  ModulePortRoleDetails,
  PortBinding
} from "../../types";

const PRIMARY_PORT_NAMES = ["master", "game", "main", "query", "game_udp", "game_tcp"];

export interface InstancePortPartition {
  player: PortBinding[];
  service: PortBinding[];
  unclassified: PortBinding[];
}

export interface InstanceNetworkControlState {
  bindDisabled: boolean;
  portsDisabled: boolean;
}

export interface InstancePortRegistrationState {
  baselinePorts: PortBinding[];
  draftPorts: PortBinding[];
}

export type InstancePortRegistrationAction =
  | {
      type: "draft-edited";
      ports: PortBinding[];
      defaultPorts: PortBinding[];
      portGroups: ModulePortGroupDetails[];
    }
  | {
      type: "baseline-received";
      ports: PortBinding[];
    };

export function normalizeInstancePorts(ports: PortBinding[]): PortBinding[] {
  return ports
    .map((port) => ({
      name: String(port.name ?? "").trim(),
      protocol: String(port.protocol ?? "").trim().toLowerCase() || "tcp",
      port: Number(port.port)
    }))
    .filter((port) => port.name.length > 0 && Number.isFinite(port.port) && port.port >= 0 && port.port <= 65535);
}

export function instancePortBindingKey(port: PortBinding): string {
  return `${String(port.name).trim()}:${String(port.protocol).trim().toLowerCase() || "tcp"}`;
}

export function sameInstancePortBinding(left: PortBinding, right: PortBinding): boolean {
  return instancePortBindingKey(left) === instancePortBindingKey(right);
}

export function alignInstancePortGroups(
  currentPorts: PortBinding[],
  nextPorts: PortBinding[],
  portGroups: ModulePortGroupDetails[]
): PortBinding[] {
  const currentByKey = new Map(currentPorts.map((port) => [instancePortBindingKey(port), port.port]));
  const aligned = normalizeInstancePorts(nextPorts);

  for (const group of portGroups) {
    const members = aligned.filter((port) => group.members.includes(port.name));
    if (members.length !== group.members.length) {
      continue;
    }
    const changed = members.filter((port) => currentByKey.get(instancePortBindingKey(port)) !== port.port);
    const source = changed.length === 1 ? changed[0] : members[0];
    const offsets = group.member_offsets ?? {};
    const maximumOffset = Math.max(0, ...members.map((member) => offsets[member.name] ?? 0));
    const sourceOffset = offsets[source.name] ?? 0;
    const minimumBasePort = maximumOffset > 0 ? 1 : 0;
    const basePort = Math.min(
      65535 - maximumOffset,
      Math.max(minimumBasePort, source.port - sourceOffset)
    );
    for (const member of members) {
      member.port = basePort + (offsets[member.name] ?? 0);
    }
  }

  return aligned;
}

export function primaryInstancePortBinding(ports: PortBinding[]): PortBinding | null {
  for (const name of PRIMARY_PORT_NAMES) {
    const match = ports.find((port) => port.name === name);
    if (match) {
      return match;
    }
  }

  return ports[0] ?? null;
}

export function orderInstancePortsForEditing(ports: PortBinding[]): PortBinding[] {
  const normalized = normalizeInstancePorts(ports);
  const primary = primaryInstancePortBinding(normalized);
  if (!primary) {
    return [];
  }

  return [primary, ...normalized.filter((port) => !sameInstancePortBinding(port, primary))];
}

export function resolveVisibleInstancePorts(
  registeredPorts: PortBinding[],
  defaultPorts: PortBinding[]
): PortBinding[] {
  const normalizedRegistered = normalizeInstancePorts(registeredPorts);
  return normalizedRegistered.length > 0 ? normalizedRegistered : normalizeInstancePorts(defaultPorts);
}

export function materializeInstancePortEdit(
  registeredPorts: PortBinding[],
  defaultPorts: PortBinding[],
  target: PortBinding,
  nextPort: number
): PortBinding[] {
  return resolveVisibleInstancePorts(registeredPorts, defaultPorts).map((port) => (
    sameInstancePortBinding(port, target) ? { ...port, port: nextPort } : port
  ));
}

export function createInstancePortRegistrationState(
  persistedPorts: PortBinding[]
): InstancePortRegistrationState {
  const normalizedPorts = normalizeInstancePorts(persistedPorts);
  return {
    baselinePorts: normalizedPorts,
    draftPorts: normalizedPorts
  };
}

export function instancePortRegistrationIsDirty(
  state: InstancePortRegistrationState
): boolean {
  return !sameInstancePorts(state.baselinePorts, state.draftPorts);
}

export function reduceInstancePortRegistrationState(
  state: InstancePortRegistrationState,
  action: InstancePortRegistrationAction
): InstancePortRegistrationState {
  if (action.type === "draft-edited") {
    const draftPorts = alignInstancePortGroups(
      resolveVisibleInstancePorts(state.draftPorts, action.defaultPorts),
      action.ports,
      action.portGroups
    );
    return sameInstancePorts(state.draftPorts, draftPorts)
      ? state
      : { ...state, draftPorts };
  }

  const baselinePorts = normalizeInstancePorts(action.ports);
  if (sameInstancePorts(state.baselinePorts, baselinePorts)) {
    return state;
  }

  const previousKeys = new Set(state.baselinePorts.map(instancePortBindingKey));
  const nextKeys = new Set(baselinePorts.map(instancePortBindingKey));
  const topologyChanged = previousKeys.size !== nextKeys.size
    || [...previousKeys].some((key) => !nextKeys.has(key));
  if (!topologyChanged) return { ...state, baselinePorts };

  // Port acknowledgements with unchanged identities must keep newer local edits.
  // A map topology change also carries newly registered endpoints from storage.
  if (!instancePortRegistrationIsDirty(state)) return { baselinePorts, draftPorts: baselinePorts };
  const draftPorts = state.draftPorts.filter((port) => !port.name.startsWith("map-") || nextKeys.has(instancePortBindingKey(port)));
  const draftKeys = new Set(draftPorts.map(instancePortBindingKey));
  draftPorts.push(...baselinePorts.filter((port) => !draftKeys.has(instancePortBindingKey(port))));
  return { baselinePorts, draftPorts };
}

export function partitionInstancePorts(
  ports: PortBinding[],
  roles: ModulePortRoleDetails[] = []
): InstancePortPartition {
  const roleByPortName = new Map<string, ModulePortRoleDetails["role"]>();
  for (const role of roles) {
    for (const portName of role.port_names) {
      const normalizedName = String(portName).trim();
      if (normalizedName && !roleByPortName.has(normalizedName)) {
        roleByPortName.set(normalizedName, role.role);
      }
    }
  }

  const partition: InstancePortPartition = { player: [], service: [], unclassified: [] };
  for (const port of ports) {
    const role = roleByPortName.get(port.name);
    if (role === "player") {
      partition.player.push(port);
    } else if (role === "service") {
      partition.service.push(port);
    } else {
      partition.unclassified.push(port);
    }
  }

  return partition;
}

function normalizeBindIp(value: string): string {
  return String(value ?? "").trim().toLowerCase() || "0.0.0.0";
}

function comparablePorts(ports: PortBinding[]): string[] {
  return normalizeInstancePorts(ports)
    .map((port) => `${instancePortBindingKey(port)}:${port.port}`)
    .sort();
}

function sameInstancePorts(left: PortBinding[], right: PortBinding[]): boolean {
  const normalizedLeft = comparablePorts(left);
  const normalizedRight = comparablePorts(right);
  return normalizedLeft.length === normalizedRight.length
    && normalizedLeft.every((port, index) => port === normalizedRight[index]);
}

export function networkDraftMatchesPersisted(
  details: Pick<InstanceDetails, "summary" | "ports">,
  draftBindIp: string,
  draftPorts: PortBinding[]
): boolean {
  if (normalizeBindIp(details.summary.bind_ip) !== normalizeBindIp(draftBindIp)) {
    return false;
  }

  return sameInstancePorts(details.ports, draftPorts);
}

export function instanceNetworkControlState(
  formDisabled: boolean,
  supportsStrictBindAddress: boolean
): InstanceNetworkControlState {
  return {
    bindDisabled: formDisabled || !supportsStrictBindAddress,
    portsDisabled: formDisabled
  };
}

export function instanceRuntimeOwnsNetworkSettings(
  details: Pick<InstanceDetails, "summary" | "active_run">
): boolean {
  const status = String(details.summary.status).trim().toLowerCase();
  return (
    details.active_run != null ||
    status === "starting" ||
    status === "running" ||
    status === "stopping"
  );
}

export function minimumInstancePortValue(role: ModulePortRole | null, isPrimary: boolean, requiredNonzero = false): 0 | 1 {
  return requiredNonzero || (role === "player" && isPrimary) ? 1 : 0;
}

export function parseInstancePortDraft(rawValue: string, minimumPort: number): number | null {
  if (!/^\d+$/.test(rawValue)) return null;
  const value = Number(rawValue);
  return Number.isInteger(value) && value >= minimumPort && value <= 65535 ? value : null;
}

export function reconcileInstancePortDrafts(
  drafts: Readonly<Record<string, string>>,
  previousPorts: readonly PortBinding[],
  nextPorts: readonly PortBinding[]
): Record<string, string> {
  const previous = new Map(previousPorts.map((port) => [instancePortBindingKey(port), port.port]));
  return Object.fromEntries(nextPorts.map((port) => {
    const key = instancePortBindingKey(port);
    const previousValue = previous.get(key);
    const draft = drafts[key];
    // An unrelated acknowledgement must not discard an unfinished port edit.
    const keepDraft = draft !== undefined && previousValue === port.port && draft !== String(previousValue);
    return [key, keepDraft ? draft : String(port.port)];
  }));
}

export function formatInstancePortName(name: string): string {
  return name
    .split(/[_\s-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}
