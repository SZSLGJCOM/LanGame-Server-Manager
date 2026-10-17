export function isInstallationCancelled(error: unknown): boolean {
  const message = error instanceof Error ? error.message : error;
  if (typeof message !== "string") return false;
  try {
    const value: unknown = JSON.parse(message);
    if (value && typeof value === "object" && "code" in value && value.code === "steam_workshop_login_cancelled") return true;
  } catch { /* Plain native cancellation messages predate structured account sessions. */ }
  return message === "installation_cancelled"
    || message.startsWith("installation_cancelled. See app log: ")
    || message.startsWith("installation_cancelled. Application diagnostic log unavailable: ") && message.includes(" Log path: ");
}
