import type { RuntimeLivePlayerEntry } from "../types";

function sameAccount(value: unknown, platform: string, userId: string): boolean {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const entry = value as Record<string, unknown>;
  return entry.platform === platform && entry.userid === userId;
}

function tenYearBanDate(executedAt: number): string {
  const date = new Date(executedAt);
  if (!Number.isFinite(date.getTime())) throw new Error("The mock ban execution date is invalid.");
  const year = date.getFullYear() + 10;
  const month = date.getMonth();
  // Match native DateTime.AddYears: February 29 clamps to February 28 in a non-leap year.
  const day = Math.min(date.getDate(), new Date(year, month + 1, 0).getDate());
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${year}-${pad(month + 1)}-${pad(day)} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

/** Synthetic persistence after the caller's authoritative snapshot execution succeeds. */
export function buildMockSevenDaysBanSettings(
  settings: Record<string, unknown>,
  player: RuntimeLivePlayerEntry | undefined,
  executedAt: number
): Record<string, unknown> {
  const identities = player?.identifiers.filter((identity) => identity.stable) ?? [];
  const identity = identities.length === 1 ? identities[0] : undefined;
  const platform = identity?.kind === "steam_id" && /^[0-9]{17}$/.test(identity.value) ? "Steam"
    : identity?.kind === "eos_id" && /^[0-9a-fA-F]{8,32}$/.test(identity.value) ? "EOS" : null;
  if (!player || !identity || !platform) {
    throw new Error("The mock action was accepted, but its canonical account cannot be synchronized.");
  }
  const blacklist = settings.blacklist_entries ?? [];
  const admins = settings.admin_users ?? [];
  if (!Array.isArray(blacklist) || !Array.isArray(admins)) {
    throw new Error("The mock action was accepted, but the existing access lists are invalid.");
  }
  const ban = {
    platform,
    userid: identity.value,
    name: player.display_name,
    unbandate: tenYearBanDate(executedAt),
    reason: "LanGame"
  };
  const existingIndex = blacklist.findIndex((entry) => sameAccount(entry, platform, identity.value));
  const nextBlacklist = [...blacklist];
  if (existingIndex === -1) nextBlacklist.push(ban);
  else nextBlacklist[existingIndex] = ban;
  return {
    ...settings,
    blacklist_entries: nextBlacklist,
    admin_users: admins.filter((entry) => !sameAccount(entry, platform, identity.value))
  };
}
