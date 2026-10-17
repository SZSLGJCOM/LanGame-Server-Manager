import type { SteamWorkshopAccountTarget } from "./steam-workshop-account-api";

export interface SteamInstallationAccountContext {
  target: SteamWorkshopAccountTarget;
  expectedAppId: number;
  loginAccount?: string;
  isCurrentOwner: () => boolean;
  requestAccount: () => Promise<string>;
}

export function requiresSteamAppAccount(reason: unknown, expectedAppId: number): boolean {
  if (!Number.isSafeInteger(expectedAppId) || expectedAppId <= 0) return false;
  let value: unknown = reason instanceof Error ? reason.message : reason;
  if (typeof value === "string") { try { value = JSON.parse(value); } catch { return false; } }
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const error = value as Record<string, unknown>;
  return error.code === "steam_app_account_required" && typeof error.message === "string"
    && typeof error.output_excerpt === "string" && typeof error.app_id === "number"
    && Number.isSafeInteger(error.app_id) && error.app_id === expectedAppId;
}

/** Only an anonymous, verified access failure may suspend and resume this operation once. */
export async function runSteamInstallationWithAccount<T>(
  anonymous: () => Promise<T>,
  authenticated: (account: string, target: SteamWorkshopAccountTarget) => Promise<T>,
  context?: SteamInstallationAccountContext
): Promise<T> {
  const assertOwner = () => { if (context && !context.isCurrentOwner()) throw new Error("installation_cancelled"); };
  assertOwner();
  if (context?.loginAccount !== undefined) return authenticated(context.loginAccount, context.target);
  try { return await anonymous(); }
  catch (reason) {
    if (!context || !requiresSteamAppAccount(reason, context.expectedAppId)) throw reason;
    assertOwner();
    const account = await context.requestAccount();
    assertOwner();
    return authenticated(account, context.target);
  }
}
