import type {
  BindAddressCandidate,
  InstanceBackupKind,
  InstanceBackupResult,
  InstanceDetails,
  InstanceRuntimeOverview,
  InstanceSummary,
  ModuleSummary,
  OverlayFamily,
  PortBinding,
  StorageStatus
} from "./types";
import { instanceHasRunningProcess } from "./runtime-action-state";
import {
  formatCoreKeeperAllowedPlatform,
  formatCoreKeeperJoinMode,
  isCoreKeeperDirectConnectionEnabled,
  parseCoreKeeperSettingsJson,
  resolveCoreKeeperAllowedPlatformCode,
  resolveCoreKeeperEffectiveGameId,
  resolveCoreKeeperJoinPassword
} from "./corekeeper-model";
import {
  NECESSE_PASSWORD_PLACEHOLDER,
  parseNecesseSettingsJson,
  resolveNecesseMaxSlots,
  resolveNecesseMotd,
  resolveNecessePassword,
  resolveNecesseWorldName
} from "./necesse-model";
import { formatDateTime, selectLocaleText, type LocaleCode, type TranslateFn } from "./i18n";
import {
  inferBindAddressCandidate,
  isWildcardBindAddress,
  normalizeBindAddressCandidates
} from "./bind-addresses";

export interface ShareEndpoint {
  address: string;
  endpoint: string;
  kind: string;
  label: string;
}

export function normalizeStatus(status: string | null | undefined): string {
  const value = String(status ?? "").trim().toLowerCase();
  if (!value) return "unknown";
  if (value.includes("run")) return "running";
  if (value.includes("stop")) return "stopped";
  if (value.includes("start")) return "starting";
  if (value.includes("error") || value.includes("fail") || value.includes("crash")) return "error";
  return value;
}

export function formatInstanceStatus(status: string | null | undefined, t: TranslateFn): string {
  switch (normalizeStatus(status)) {
    case "running":
      return t("status.instance.running");
    case "stopped":
      return t("status.instance.stopped");
    case "starting":
      return t("status.instance.starting");
    case "stopping":
      return t("status.instance.stopping");
    case "error":
      return t("status.instance.error");
    default:
      return status || t("status.instance.unknown");
  }
}

export function isRunningStatus(status: string | null | undefined): boolean {
  return normalizeStatus(status) === "running";
}

export function countRunningInstances(instances: InstanceSummary[]): number {
  return instances.filter((instance) => instanceHasRunningProcess(instance)).length;
}

export function primaryPort(details: Pick<InstanceDetails, "ports"> | null | undefined): PortBinding | null {
  const ports = details?.ports ?? [];
  if (!ports.length) return null;

  const preferredNames = ["master", "game", "main", "query", "game_udp", "game_tcp"];
  for (const name of preferredNames) {
    const match = ports.find((port) => port.name === name);
    if (match) return match;
  }

  return ports[0] ?? null;
}

function portBindingByName(details: InstanceDetails, name: string, protocol?: string): PortBinding | null {
  const expectedName = name.toLowerCase();
  const expectedProtocol = protocol?.toLowerCase();
  return details.ports.find((port) => (
    port.name.toLowerCase() === expectedName
    && (!expectedProtocol || String(port.protocol).toLowerCase() === expectedProtocol)
  )) ?? null;
}

function formatInvitePort(port: PortBinding | null): string {
  return port ? `${String(port.protocol).toUpperCase()} ${port.port}` : "not declared";
}

function buildVrisingInviteNotes(details: InstanceDetails, locale: LocaleCode): string[] {
  const gamePort = portBindingByName(details, "game", "udp");
  const queryPort = portBindingByName(details, "query", "udp");
  return [
    selectLocaleText(
      locale,
      `V Rising \u5217\u8868\u53d1\u73b0\u8d70 Query ${formatInvitePort(queryPort)}\uff1b\u771f\u6b63\u5165\u670d\u8d70 Game ${formatInvitePort(gamePort)}\u3002`,
      `V Rising browser discovery uses Query ${formatInvitePort(queryPort)}; actual joining uses Game ${formatInvitePort(gamePort)}.`
    ),
    selectLocaleText(
      locale,
      `\u5982\u679c\u76f4\u8fde\u80fd\u8fdb\u3001\u5217\u8868\u5165\u53e3\u4e0d\u80fd\u8fdb\uff0c\u8bf7\u4f18\u5148\u53d1\u8fd9\u4e2a\u76f4\u8fde\u5730\u5740\uff1b\u670d\u52a1\u5668\u5217\u8868\u53ea\u662f\u53d1\u73b0\u5165\u53e3\uff0c\u4e0d\u4fdd\u8bc1\u4f1a\u6cbf\u540c\u4e00\u6761\u5c40\u57df\u7f51/\u8fdc\u7a0b\u5c40\u57df\u7f51\u8def\u5f84\u8fde\u63a5\u3002`,
      "If direct join works but browser entry does not, share this direct endpoint first; the server browser is only a discovery entry and does not guarantee the same LAN/remote-LAN route."
    )
  ];
}

export function moduleNameFromId(moduleId: string, modules: ModuleSummary[]): string {
  return modules.find((module) => module.id === moduleId)?.name ?? moduleId;
}

function shareEndpointPriority(candidate: BindAddressCandidate): number {
  switch (String(candidate.kind ?? "").toLowerCase()) {
    case "public":
      return 0;
    case "overlay":
      return 1;
    case "lan":
      return 2;
    case "configured":
      return 3;
    case "all":
      return 4;
    case "proxy":
      return 9;
    default:
      return 5;
  }
}

function formatShareLabel(candidate: BindAddressCandidate, locale: LocaleCode): string {
  const adapterName = String(candidate.adapter_name ?? "").trim();
  const familyName = String(candidate.family_name ?? "").trim();

  switch (String(candidate.kind ?? "").toLowerCase()) {
    case "overlay":
      return familyName || adapterName || selectLocaleText(locale, "\u8054\u673a\u7f51\u7edc", "Overlay network");
    case "public":
      return selectLocaleText(locale, "\u516c\u7f51 IPv4", "Public IPv4");
    case "proxy":
      return selectLocaleText(locale, "\u4ee3\u7406/TUN \u865a\u62df\u7f51\u5361", "Proxy/TUN virtual adapter");
    case "lan":
      if (adapterName) {
        return `${selectLocaleText(locale, "\u5c40\u57df\u7f51", "LAN")} / ${adapterName}`;
      }
      return selectLocaleText(locale, "\u5c40\u57df\u7f51", "LAN");
    case "configured":
      return selectLocaleText(locale, "\u5df2\u4fdd\u5b58\u5730\u5740", "Saved address");
    case "all":
      return selectLocaleText(locale, "\u76d1\u542c\u5168\u90e8\u7f51\u5361", "All adapters");
    default:
      return candidate.address;
  }
}

function resolveShareCandidates(
  details: Pick<InstanceDetails, "summary"> | null | undefined,
  bindAddressCandidates: BindAddressCandidate[]
): BindAddressCandidate[] {
  if (!details) {
    return [];
  }

  const configuredBindIp = String(details.summary.bind_ip ?? "").trim();
  const normalizedCandidates = normalizeBindAddressCandidates(bindAddressCandidates);

  if (!isWildcardBindAddress(configuredBindIp)) {
    const exact = normalizedCandidates.find((candidate) => candidate.address === configuredBindIp);
    return [exact ?? inferBindAddressCandidate(configuredBindIp)];
  }

  const visibleCandidates = normalizedCandidates.filter((candidate) => !isWildcardBindAddress(candidate.address));
  const shareableCandidates = visibleCandidates.filter((candidate) => (
    String(candidate.kind ?? "").trim().toLowerCase() !== "proxy"
  ));
  if (shareableCandidates.length > 0) {
    return shareableCandidates;
  }

  return [inferBindAddressCandidate(configuredBindIp || "0.0.0.0")];
}

export function buildShareEndpoints(
  details: Pick<InstanceDetails, "summary" | "ports" | "settings_json"> | null | undefined,
  bindAddressCandidates: BindAddressCandidate[],
  locale: LocaleCode,
  t: TranslateFn
): ShareEndpoint[] {
  if (details?.summary.module_id === "corekeeper") {
    const settings = parseCoreKeeperSettingsJson(details.settings_json);
    if (!isCoreKeeperDirectConnectionEnabled(settings)) {
      const effectiveGameId = resolveCoreKeeperEffectiveGameId(settings, details.summary.id);
      return [
        {
          address: effectiveGameId,
          endpoint: effectiveGameId,
          kind: "relay",
          label: t("servers.corekeeper.share.relay", undefined, "Steam relay Game ID")
        }
      ];
    }
  }

  const port = primaryPort(details);
  const pendingPort = t("messages.pendingPort");

  return resolveShareCandidates(details, bindAddressCandidates)
    .sort((left, right) => shareEndpointPriority(left) - shareEndpointPriority(right))
    .map((candidate) => ({
      address: candidate.address,
      endpoint: `${candidate.address.includes(":") && !candidate.address.startsWith("[") ? `[${candidate.address}]` : candidate.address}:${port ? port.port : pendingPort}`,
      kind: String(candidate.kind ?? "").trim().toLowerCase() || "configured",
      label: formatShareLabel(candidate, locale)
    }));
}

function resolveCoreKeeperRoomName(details: InstanceDetails): string {
  const settings = parseCoreKeeperSettingsJson(details.settings_json);
  const configuredName = String(settings.server_name ?? "").trim();
  return configuredName || details.summary.name;
}

export function buildCoreKeeperInviteText(
  details: InstanceDetails,
  bindAddressCandidates: BindAddressCandidate[],
  locale: LocaleCode,
  t: TranslateFn
): string {
  const settings = parseCoreKeeperSettingsJson(details.settings_json);
  const directConnectionEnabled = isCoreKeeperDirectConnectionEnabled(settings);
  const roomName = resolveCoreKeeperRoomName(details);
  const shareEndpoints = buildShareEndpoints(details, bindAddressCandidates, locale, t);
  const primaryEndpoint = shareEndpoints[0]?.endpoint ?? buildJoinEndpoint(details, bindAddressCandidates, locale, t);
  const additionalEndpoints = directConnectionEnabled ? shareEndpoints.slice(1) : [];
  const joinPassword = resolveCoreKeeperJoinPassword(settings);

  return [
    t("servers.corekeeper.invite.header", { name: roomName }, `Core Keeper: ${roomName}`),
    `${t("servers.corekeeper.invite.joinMode", undefined, "Join mode")}: ${formatCoreKeeperJoinMode(directConnectionEnabled, t)}`,
    directConnectionEnabled
      ? `${t("servers.corekeeper.invite.primaryJoin", undefined, "Primary join")}: ${primaryEndpoint}`
      : `${t("servers.corekeeper.invite.gameId", undefined, "Steam relay Game ID")}: ${resolveCoreKeeperEffectiveGameId(settings, details.summary.id)}`,
    ...additionalEndpoints.map((endpoint) => `${endpoint.label}: ${endpoint.endpoint}`),
    directConnectionEnabled
      ? `${t("servers.corekeeper.invite.password", undefined, "Join password")}: ${joinPassword || t("servers.corekeeper.invite.passwordMissing", undefined, "Not set")}`
      : null,
    directConnectionEnabled
      ? `${t("servers.corekeeper.invite.allowedPlatform", undefined, "Allowed platform")}: ${formatCoreKeeperAllowedPlatform(
          resolveCoreKeeperAllowedPlatformCode(settings),
          t
        )}`
      : null,
    `${t("servers.corekeeper.invite.howToJoin", undefined, "How to join")}: ${t(
      directConnectionEnabled
        ? "servers.corekeeper.invite.instructionsDirect"
        : "servers.corekeeper.invite.instructionsRelay",
      undefined,
      directConnectionEnabled
        ? "Open Multiplayer, choose Direct IP, then enter the endpoint and password above."
        : "Open Multiplayer, choose Join Game ID, then enter the Game ID above."
    )}`
  ]
    .filter((line): line is string => Boolean(line))
    .join("\n");
}

export function buildNecesseInviteText(
  details: InstanceDetails,
  bindAddressCandidates: BindAddressCandidate[],
  locale: LocaleCode,
  t: TranslateFn
): string {
  const settings = parseNecesseSettingsJson(details.settings_json);
  const shareEndpoints = buildShareEndpoints(details, bindAddressCandidates, locale, t);
  const primaryEndpoint = shareEndpoints[0]?.endpoint ?? buildJoinEndpoint(details, bindAddressCandidates, locale, t);
  const password = resolveNecessePassword(settings);
  const worldName = resolveNecesseWorldName(settings, details.summary.name);
  const maxSlots = resolveNecesseMaxSlots(settings);
  const motd = resolveNecesseMotd(settings);

  return [
    t("servers.necesse.invite.header", { name: worldName }, `Necesse: ${worldName}`),
    `${t("servers.necesse.invite.address", undefined, "Address")}: ${primaryEndpoint}`,
    ...shareEndpoints.slice(1).map((endpoint) => `${endpoint.label}: ${endpoint.endpoint}`),
    `${t("servers.necesse.invite.password", undefined, "Join password")}: ${password || t("servers.necesse.valueOpen", undefined, "Open")}`,
    `${t("servers.necesse.invite.slots", undefined, "Player slots")}: ${maxSlots ?? "-"}`,
    password === NECESSE_PASSWORD_PLACEHOLDER
      ? t(
          "servers.necesse.invite.passwordPlaceholderWarning",
          undefined,
          "This room is still using the placeholder password. Replace it before daily use."
        )
      : null,
    motd
      ? `${t("servers.necesse.invite.motd", undefined, "Message of the day")}: ${motd}`
      : null
  ]
    .filter((line): line is string => Boolean(line))
    .join("\n");
}

export function buildInviteText(
  details: InstanceDetails,
  modules: ModuleSummary[],
  locale: LocaleCode,
  t: TranslateFn,
  bindAddressCandidates: BindAddressCandidate[]
): string {
  const port = primaryPort(details);
  const shareEndpoints = buildShareEndpoints(details, bindAddressCandidates, locale, t);
  const game = moduleNameFromId(details.summary.module_id, modules);
  const moduleNotes = details.summary.module_id === "vrising"
    ? buildVrisingInviteNotes(details, locale)
    : [];

  if (details.summary.module_id === "corekeeper") {
    return buildCoreKeeperInviteText(details, bindAddressCandidates, locale, t);
  }

  if (details.summary.module_id === "necesse") {
    return buildNecesseInviteText(details, bindAddressCandidates, locale, t);
  }

  return [
    `${game}: ${details.summary.name}`,
    `${t("messages.addressLabel")}: ${shareEndpoints[0]?.endpoint ?? buildJoinEndpoint(details, bindAddressCandidates, locale, t)}`,
    ...shareEndpoints.slice(1).map((endpoint) => `${endpoint.label}: ${endpoint.endpoint}`),
    `${t("messages.portLabel")}: ${port ? `${String(port.protocol).toUpperCase()} ${port.port}` : "-"}`,
    ...moduleNotes,
    t("messages.inviteShareTip")
  ].join("\n");
}

export function buildJoinEndpoint(
  details: InstanceDetails | null | undefined,
  bindAddressCandidates: BindAddressCandidate[],
  locale: LocaleCode,
  t: TranslateFn
): string {
  const primaryEndpoint = buildShareEndpoints(details, bindAddressCandidates, locale, t)[0];
  if (primaryEndpoint) {
    return primaryEndpoint.endpoint;
  }

  const port = primaryPort(details);
  const fallbackAddress = String(details?.summary.bind_ip ?? "").trim()
    || normalizeBindAddressCandidates(bindAddressCandidates)[0]?.address
    || "0.0.0.0";
  if (!port) return `${fallbackAddress}:${t("messages.pendingPort")}`;
  return `${fallbackAddress}:${port.port}`;
}

export function buildJoinHint(details: InstanceDetails | null | undefined, t: TranslateFn): string {
  const port = primaryPort(details);
  if (!port) {
    return t("messages.createInstanceFirst");
  }
  return `${String(port.protocol).toUpperCase()} ${port.port}`;
}

export function formatPorts(ports: PortBinding[], t: TranslateFn): string {
  if (!ports.length) {
    return t("common.noPorts");
  }
  return ports.map((port) => `${port.name}: ${String(port.protocol).toUpperCase()} ${port.port}`).join(" / ");
}

export function formatBackupKind(kind: InstanceBackupKind, t: TranslateFn): string {
  switch (kind) {
    case "auto_stop":
      return t("servers.backups.kind.auto_stop");
    case "pre_restore":
      return t("servers.backups.kind.pre_restore");
    case "manual":
    default:
      return t("servers.backups.kind.manual");
  }
}

export function latestBackupByKind(
  backups: InstanceBackupResult[],
  kind: InstanceBackupKind
): InstanceBackupResult | null {
  return backups.find((backup) => backup.backup_kind === kind) ?? null;
}

export function buildBackupPolicySummary(details: InstanceDetails, t: TranslateFn): string {
  return details.auto_backup_on_stop
    ? t("servers.details.backupPolicyEnabled", { count: details.backup_retention_count })
    : t("servers.details.backupPolicyDisabled", { count: details.backup_retention_count });
}

export function overlaySummary(overlays: OverlayFamily[], t: TranslateFn): string {
  const names = overlays.map((overlay) => overlay.name).filter(Boolean);
  return names.length ? names.join(" / ") : t("common.noOverlay");
}

export function overlayMatchSummary(overlay: OverlayFamily, t: TranslateFn): string {
  const patterns = [
    ...(overlay.adapter_name_patterns ?? []),
    ...(overlay.executable_name_patterns ?? [])
  ].filter(Boolean);

  return patterns.length ? patterns.join(", ") : t("common.notProvided");
}

export function storageIsReady(storage: StorageStatus): boolean {
  return Boolean(storage.database_exists && storage.migrations_applied);
}

export function storageStatusCopy(storage: StorageStatus, t: TranslateFn): string {
  if (storageIsReady(storage)) {
    return t("messages.storageReady", { version: storage.schema_version ?? 0 });
  }
  return t("messages.storagePending");
}

export function formatTime(locale: LocaleCode, value: string | number | Date | null | undefined): string {
  return formatDateTime(locale, value, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit"
  });
}

export function formatRuntimeAge(
  value: string | number | Date | null | undefined,
  t: TranslateFn
): string {
  if (!value) return t("messages.timeJustNow");
  const date = value instanceof Date ? value : new Date(value);
  const diffMs = Math.max(0, Date.now() - date.getTime());
  const diffMinutes = Math.floor(diffMs / 60000);

  if (diffMinutes < 1) return t("messages.timeJustNow");
  if (diffMinutes < 60) return t("messages.timeMinutes", { count: diffMinutes });

  const diffHours = Math.floor(diffMinutes / 60);
  if (diffHours < 24) return t("messages.timeHours", { count: diffHours });

  const diffDays = Math.floor(diffHours / 24);
  return t("messages.timeDays", { count: diffDays });
}

export function latestRun(runtime: InstanceRuntimeOverview | null | undefined) {
  return runtime?.recent_runs[0] ?? null;
}

export function buildRuntimeMeta(
  instance: InstanceSummary,
  runtime: InstanceRuntimeOverview | null | undefined,
  locale: LocaleCode,
  t: TranslateFn
): string {
  const latest = latestRun(runtime);
  if (!latest) return t("messages.runtimeWaitingFirstStart");
  if (isRunningStatus(latest.status) && latest.started_at) {
    return t("messages.runtimeRunningSince", { value: formatRuntimeAge(latest.started_at, t) });
  }
  if (latest.stopped_at) {
    return t("messages.runtimeLastStopped", { value: formatTime(locale, latest.stopped_at) });
  }
  return formatInstanceStatus(latest.status || instance.status, t);
}

export function latestLogPath(
  details: InstanceDetails | null | undefined,
  runtime: InstanceRuntimeOverview | null | undefined,
  t: TranslateFn
): string {
  return details?.active_run?.log_path ?? latestRun(runtime)?.log_path ?? t("messages.noLogFile");
}

export function processCount(
  details: InstanceDetails | null | undefined,
  runtime: InstanceRuntimeOverview | null | undefined
): number {
  return latestRun(runtime)?.process_count ?? details?.active_run?.process_count ?? 0;
}
