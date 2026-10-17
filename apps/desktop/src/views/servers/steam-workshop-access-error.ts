/** Only the verified native contract may interrupt an install for account login. */
export function requiresSteamWorkshopAccount(error: unknown): boolean {
  let value: unknown = error instanceof Error ? error.message : error;
  if (typeof value === "string") {
    try { value = JSON.parse(value); } catch { return false; }
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const record = value as Record<string, unknown>;
  return (record.code === "steam_workshop_decryption_key_missing" || record.code === "steam_workshop_account_required") && typeof record.message === "string"
    && typeof record.output_excerpt === "string" && typeof record.consumer_app_id === "number"
    && Number.isSafeInteger(record.consumer_app_id) && record.consumer_app_id > 0;
}
