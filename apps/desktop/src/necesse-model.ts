import type { TranslateFn } from "./i18n";
import type { InstanceDetails } from "./types";

export const NECESSE_OWNER_PLACEHOLDER = "ChangeMeOwner";
export const NECESSE_PASSWORD_PLACEHOLDER = "change-me-necesse";
export const NECESSE_PERMISSION_LEVELS = ["USER", "MODERATOR", "ADMIN", "OWNER"] as const;

export type NecessePermissionLevel = (typeof NECESSE_PERMISSION_LEVELS)[number];

export interface NecesseConsoleCommandBlock {
  command: string;
  submitted_at_label: string | null;
  output_lines: string[];
}

export interface NecesseStoredIdentity {
  authentication: string;
  names: string[];
  raw_line: string;
}

function readString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function readBoolean(value: unknown): boolean {
  if (typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    return value !== 0;
  }
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (["true", "1", "yes", "on"].includes(normalized)) {
      return true;
    }
    if (["false", "0", "no", "off", ""].includes(normalized)) {
      return false;
    }
  }
  return Boolean(value);
}

function readNumber(value: unknown): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export function parseNecesseSettingsJson(settingsJson: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(settingsJson) as unknown;
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : {};
  } catch {
    return {};
  }
}

export function resolveNecesseWorldName(
  settings: Record<string, unknown>,
  fallbackName: string
): string {
  return readString(settings.world_name).trim() || fallbackName;
}

export function resolveNecesseMotd(settings: Record<string, unknown>): string {
  return readString(settings.motd).trim();
}

export function resolveNecesseOwnerName(settings: Record<string, unknown>): string {
  return readString(settings.owner_name).trim();
}

export function isNecesseOwnerNameRuntimeUnsafe(value: string): boolean {
  return readString(value).trim().includes("-");
}

export function resolveNecessePassword(settings: Record<string, unknown>): string {
  return readString(settings.password).trim();
}

export function resolveNecesseLanguage(settings: Record<string, unknown>): string {
  return readString(settings.language).trim() || "en";
}

export function resolveNecesseMaxSlots(settings: Record<string, unknown>): number | null {
  return readNumber(settings.max_slots);
}

export function isNecessePauseWhenEmpty(settings: Record<string, unknown>): boolean {
  return readBoolean(settings.pause_when_empty);
}

export function isNecesseStrictServerAuthority(settings: Record<string, unknown>): boolean {
  return readBoolean(settings.strict_server_authority);
}

export function isNecesseLoggingEnabled(settings: Record<string, unknown>): boolean {
  return readBoolean(settings.logging_enabled);
}

export function isNecesseZipSavesEnabled(settings: Record<string, unknown>): boolean {
  return readBoolean(settings.zip_saves);
}

export function isNecesseIgnoreSeasons(settings: Record<string, unknown>): boolean {
  return readBoolean(settings.ignore_seasons);
}

export function readNecessePort(details: InstanceDetails, portName = "game"): number | null {
  return details.ports.find((port) => port.name === portName)?.port ?? null;
}

export function formatNecesseSecretState(
  value: string,
  t: TranslateFn,
  options?: {
    placeholder?: string;
    openWhenEmpty?: boolean;
  }
): string {
  const trimmed = readString(value).trim();
  if (!trimmed) {
    return options?.openWhenEmpty
      ? t("servers.necesse.valueOpen", undefined, "Open")
      : t("servers.necesse.valueNotSet", undefined, "Not set");
  }
  if (options?.placeholder && trimmed === options.placeholder) {
    return t("servers.necesse.valuePlaceholder", undefined, "Placeholder");
  }
  return t("servers.necesse.valueConfigured", undefined, "Configured");
}

function stripNecesseRuntimeLogPrefix(line: string): string {
  return readString(line)
    .trim()
    .replace(/^\[[^\]]+\]\s*/, "")
    .trim();
}

export function parseNecesseConsoleCommandBlocks(lines: string[]): NecesseConsoleCommandBlock[] {
  const blocks: NecesseConsoleCommandBlock[] = [];
  let currentBlock: NecesseConsoleCommandBlock | null = null;

  for (const rawLine of lines) {
    const trimmedLine = readString(rawLine).trim();
    if (!trimmedLine) {
      continue;
    }

    const liveCommandMatch = trimmedLine.match(/^\[([^\]]+)\]\s+>\s+(.+)$/);
    if (liveCommandMatch) {
      if (currentBlock) {
        blocks.push(currentBlock);
      }
      currentBlock = {
        command: liveCommandMatch[2].trim(),
        submitted_at_label: liveCommandMatch[1].trim() || null,
        output_lines: []
      };
      continue;
    }

    const mockCommandMatch = trimmedLine.match(/^\[console:[^\]]+\]\s+(.+)$/i);
    if (mockCommandMatch) {
      if (currentBlock) {
        blocks.push(currentBlock);
      }
      currentBlock = {
        command: mockCommandMatch[1].trim(),
        submitted_at_label: null,
        output_lines: []
      };
      continue;
    }

    if (!currentBlock) {
      continue;
    }

    const normalizedOutput = stripNecesseRuntimeLogPrefix(trimmedLine);
    if (normalizedOutput) {
      currentBlock.output_lines.push(normalizedOutput);
    }
  }

  if (currentBlock) {
    blocks.push(currentBlock);
  }

  return blocks;
}

export function findLatestNecesseConsoleCommandBlock(
  blocks: NecesseConsoleCommandBlock[],
  commandPrefixes: string | string[]
): NecesseConsoleCommandBlock | null {
  const prefixes = (Array.isArray(commandPrefixes) ? commandPrefixes : [commandPrefixes])
    .map((prefix) => readString(prefix).trim().toLowerCase())
    .filter(Boolean);

  if (prefixes.length === 0) {
    return null;
  }

  for (let index = blocks.length - 1; index >= 0; index -= 1) {
    const command = readString(blocks[index]?.command).trim().toLowerCase();
    if (!command) {
      continue;
    }

    if (prefixes.some((prefix) => command === prefix || command.startsWith(`${prefix} `))) {
      return blocks[index] ?? null;
    }
  }

  return null;
}

function normalizeNecesseIdentityLine(line: string): string {
  return readString(line).trim();
}

export function parseNecesseStoredIdentities(commandBlock: NecesseConsoleCommandBlock | null): NecesseStoredIdentity[] {
  if (!commandBlock) {
    return [];
  }

  const entries: NecesseStoredIdentity[] = [];
  for (const rawLine of commandBlock.output_lines) {
    const line = normalizeNecesseIdentityLine(rawLine);
    if (!line || /^Total players stored:/i.test(line)) {
      continue;
    }

    let authentication = line;
    let names: string[] = [];

    if (line.includes(" -> ")) {
      const [left, right] = line.split(/\s+->\s+/, 2);
      authentication = readString(left).trim() || line;
      names = readString(right)
        .split(/\s*,\s*/)
        .map((value) => value.trim())
        .filter(Boolean);
    } else {
      const colonMatch = line.match(/^([^:]+):\s+(.+)$/);
      const parenMatch = line.match(/^(.+?)\s+\((.+)\)$/);
      if (colonMatch) {
        authentication = colonMatch[1].trim() || line;
        names = colonMatch[2]
          .split(/\s*,\s*/)
          .map((value) => value.trim())
          .filter(Boolean);
      } else if (parenMatch) {
        authentication = parenMatch[1].trim() || line;
        names = parenMatch[2]
          .split(/\s*,\s*/)
          .map((value) => value.trim())
          .filter(Boolean);
      }
    }

    entries.push({
      authentication,
      names,
      raw_line: line
    });
  }

  return entries;
}

export function parseNecesseBanEntries(commandBlock: NecesseConsoleCommandBlock | null): string[] {
  if (!commandBlock) {
    return [];
  }

  return commandBlock.output_lines
    .map((line) => normalizeNecesseIdentityLine(line))
    .filter((line) => Boolean(line))
    .filter((line) => !/^There are no listed bans\./i.test(line))
    .filter((line) => !/^\d+\s+total bans:/i.test(line));
}

export function parseNecessePermissionLevelCatalog(commandBlock: NecesseConsoleCommandBlock | null): string[] {
  if (!commandBlock) {
    return [];
  }

  return commandBlock.output_lines
    .flatMap((line) => normalizeNecesseIdentityLine(line).split(/\s*,\s*/))
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0)
    .filter((entry) => !/^Permission levels:?$/i.test(entry));
}
