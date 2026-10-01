function uniqueEntries(values: string[]): string[] {
  const seen = new Set<string>();
  const entries: string[] = [];
  for (const value of values.map((entry) => entry.trim()).filter(Boolean)) {
    const key = value.toLowerCase();
    if (!seen.has(key)) {
      seen.add(key);
      entries.push(value);
    }
  }
  return entries;
}

function isModReferenceCandidate(token: string): boolean {
  return /^https?:\/\//i.test(token) ||
    /^\d{5,}$/.test(token) ||
    /^(?:modrinth|mr):[A-Za-z0-9_-]{2,64}$/i.test(token);
}

function decodeHtmlEntities(value: string): string {
  return value
    .replace(/&amp;/gi, "&")
    .replace(/&quot;/gi, "\"")
    .replace(/&#39;|&apos;/gi, "'")
    .replace(/&lt;/gi, "<")
    .replace(/&gt;/gi, ">");
}

function unwrapRedirectReference(value: string): string {
  let current = value;
  for (let attempt = 0; attempt < 4; attempt += 1) {
    let parsed: URL;
    try {
      parsed = new URL(current);
    } catch {
      return current;
    }

    const target = ["u", "url", "q", "target", "redirect", "redirect_url"]
      .map((key) => parsed.searchParams.get(key))
      .find((candidate): candidate is string => Boolean(candidate && /^https?:\/\//i.test(candidate)));
    if (!target || target === current) {
      return current;
    }
    current = target;
  }
  return current;
}

function normalizeReferenceCandidate(token: string): string {
  const decoded = decodeHtmlEntities(token.trim().replace(/[),;\]}]+$/g, ""));
  return /^https?:\/\//i.test(decoded) ? unwrapRedirectReference(decoded) : decoded;
}

export function referenceCandidatesFromText(text: string): string[] {
  if (!text.trim()) {
    return [];
  }
  const hrefs = Array.from(text.matchAll(/href=["']([^"']+)["']/gi)).map((match) => normalizeReferenceCandidate(match[1]));
  const tokens = text
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split(/[\s"'<>]+/)
    .map(normalizeReferenceCandidate)
    .filter(Boolean);
  return uniqueEntries([...hrefs, ...tokens].filter(isModReferenceCandidate));
}
