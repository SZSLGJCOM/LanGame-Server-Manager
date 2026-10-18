/** Match the ASE ActiveMods renderer and workshop_ids::normalize_reference. */
export function parseEvolvedModIds(value: unknown): string[] {
  if (typeof value !== "string") return [];
  const ids = new Set<string>();
  // Rust char::is_whitespace includes NEL and excludes the JavaScript-only BOM.
  for (const token of value.split(/[\u0009-\u000d\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000,;、，]+/)) {
    if (!token || token.startsWith("#") || token.startsWith("--")) continue;
    const normalized = token.startsWith("workshop-") ? token.slice(9) : token;
    const referenceIndex = token.indexOf("id="), referenceId = referenceIndex < 0 ? undefined
      : token.slice(referenceIndex + 3).match(/^[0-9]{6,}/)?.[0];
    const id = /^[0-9]{6,}$/.test(normalized) ? normalized
      : referenceId ?? token.match(/[0-9]{6,}/)?.[0];
    if (id) ids.add(id);
  }
  return [...ids];
}
