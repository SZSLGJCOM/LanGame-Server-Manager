/** Keep signed Nexus references intact in memory; the native boundary validates identity and game. */
export function nexusReferencesFromText(text: string): string[] {
  return [...new Set(text.split(/[\s"'<>]+/).map((token) => token.replace(/&amp;/gi, "&"))
    .filter((token) => /^(?:https?:\/\/|nxm:\/\/)/i.test(token) || /^[0-9]+$/.test(token)))];
}
