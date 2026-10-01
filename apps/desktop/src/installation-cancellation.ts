export function isInstallationCancelled(error: unknown): boolean {
  const message = error instanceof Error ? error.message : error;
  if (typeof message !== "string") return false;
  return message === "installation_cancelled"
    || message.startsWith("installation_cancelled. See app log: ")
    || message.startsWith("installation_cancelled. Application diagnostic log unavailable: ") && message.includes(" Log path: ");
}
