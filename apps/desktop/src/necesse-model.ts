export const NECESSE_OWNER_PLACEHOLDER = "ChangeMeOwner";
export const NECESSE_PASSWORD_PLACEHOLDER = "change-me-necesse";
function readString(value: unknown): string {
  return typeof value === "string" ? value : "";
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

export function isNecesseOwnerNameRuntimeUnsafe(value: string): boolean {
  return readString(value).trim().includes("-");
}

export function resolveNecessePassword(settings: Record<string, unknown>): string {
  return readString(settings.password).trim();
}

export function resolveNecesseMaxSlots(settings: Record<string, unknown>): number | null {
  return readNumber(settings.max_slots);
}
