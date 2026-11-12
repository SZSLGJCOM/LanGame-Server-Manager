/** Keep signed Nexus references intact in memory; the native boundary validates identity and game. */
export function nexusReferencesFromText(text: string): string[] {
  return [...new Set(text.split(/[\s"'<>]+/).map((token) => token.replace(/&amp;/gi, "&"))
    .filter((token) => /^(?:https?:\/\/|nxm:\/\/)/i.test(token) || /^[0-9]+$/.test(token)))];
}

/** A dragged card can expose its image as the URI and its project as an HTML link. */
export function nexusReferencesFromDrop(transfer: Pick<DataTransfer, "getData">): string[] {
  const uri = nexusReferencesFromText(transfer.getData("text/uri-list"));
  const plain = nexusReferencesFromText(transfer.getData("text/plain"));
  const html = transfer.getData("text/html");
  const hrefs = Array.from(html.matchAll(/\bhref\s*=\s*["']([^"']+)["']/gi), (match) => match[1]);
  const links = nexusReferencesFromText(hrefs.length ? hrefs.join("\n") : html);
  // Pick one representation, excluding thumbnail/CDN URLs and numbers from
  // captions. Keep the original reference bytes; native code validates it.
  for (const references of [uri, links, plain]) {
    const projects = references.filter((reference) =>
      /^https?:\/\/(?:www\.)?nexusmods\.com\/(?:games\/)?[a-z0-9_-]+\/mods\/[0-9]+(?:[/?#]|$)/i.test(reference)
      || /^nxm:\/\/[a-z0-9_-]+\/mods\/[0-9]+\/files\/[0-9]+(?:[/?#]|$)/i.test(reference));
    if (projects.length) return projects;
  }
  // Preserve short IDs and invalid inputs for the existing native error path.
  return uri.length ? uri : plain.length ? plain : links;
}
