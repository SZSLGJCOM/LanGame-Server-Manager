(() => {
  // Read only public project metadata. Native code independently verifies the
  // observed WebView URL and the returned project identity before caching it.
  const result = { url: "", canonicalUrl: null, projectId: null, title: null, state: "pending" };
  const normalizeText = (value) => value.replace(/\s+/g, " ").trim();
  const projectUrl = (value) => {
    if (typeof value !== "string" || value.length > 2048) return null;
    let parsed;
    try { parsed = new URL(value, location.href); } catch { return null; }
    if (parsed.protocol !== "https:" || parsed.username || parsed.password || parsed.port
      || !["curseforge.com", "www.curseforge.com"].includes(parsed.hostname)) return null;
    const match = parsed.pathname.match(/^\/ark-survival-ascended\/mods\/([a-z0-9_-]{1,128})(?:\/files(?:\/\d{1,10})?)?\/?$/i);
    if (!match) return null;
    // Challenge query parameters may contain temporary verification tokens.
    return { url: parsed.origin + parsed.pathname, slug: match[1].toLowerCase() };
  };
  const current = projectUrl(location.href);
  if (!current) return { ...result, state: "invalid" };
  result.url = current.url;

  const canonicalLinks = document.querySelectorAll('link[rel~="canonical"]');
  if (canonicalLinks.length > 4) return { ...result, state: "invalid" };
  for (const link of canonicalLinks) {
    const canonical = projectUrl(link.getAttribute("href"));
    if (!canonical || canonical.slug !== current.slug) return { ...result, state: "invalid" };
    result.canonicalUrl = canonical.url;
  }

  const challengeTitle = /^(?:just a moment|attention required|security verification|verify (?:you are|you're) human|请稍候)/i;
  if (challengeTitle.test(document.title.slice(0, 240))
    || document.querySelector('#challenge-running, #challenge-stage, #challenge-form, #cf-chl-widget, .cf-error-details')) {
    return { ...result, state: "challenge" };
  }

  const headings = document.querySelectorAll("h1");
  if (headings.length > 4) return { ...result, state: "invalid" };
  const titles = new Set();
  for (const heading of headings) {
    if (heading.textContent.length > 1024) return { ...result, state: "invalid" };
    const title = normalizeText(heading.textContent);
    if (title) titles.add(title);
  }
  if (titles.size > 1) return { ...result, state: "invalid" };
  result.title = titles.values().next().value?.slice(0, 240) ?? null;

  const lists = document.querySelectorAll("dl");
  if (lists.length > 128) return { ...result, state: "invalid" };
  const ids = new Set();
  for (const list of lists) {
    // Author description/comments can contain arbitrary markup; they cannot
    // establish the project's identity even when quoting another project.
    if (list.closest('.project-description, .project-comment, #description, #comments, .wysiwyg')) continue;
    const terms = [...list.querySelectorAll("dt")].filter((term) => term.closest("dl") === list);
    if (terms.length > 32) return { ...result, state: "invalid" };
    const labels = terms.map((term) => normalizeText(term.textContent).toLowerCase());
    const metadataLabels = ["downloads", "created", "updated", "license"];
    // Require the project's Details metadata, not a stray label in a file
    // description or a recommendation card.
    if (metadataLabels.filter((label) => labels.includes(label)).length < 2) continue;
    for (let index = 0; index < terms.length; index += 1) {
      if (labels[index] !== "project id") continue;
      const value = terms[index].nextElementSibling;
      if (!value || value.tagName !== "DD") return { ...result, state: "invalid" };
      const id = normalizeText(value.textContent);
      if (!/^[1-9]\d{0,9}$/.test(id) || Number(id) > 4294967295) return { ...result, state: "invalid" };
      ids.add(id);
    }
  }
  if (ids.size > 1) return { ...result, state: "invalid" };
  if (!result.canonicalUrl || !result.title || ids.size !== 1) return result;
  return { ...result, projectId: ids.values().next().value, state: "ready" };
})();
