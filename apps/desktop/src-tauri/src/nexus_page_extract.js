(() => {
  // Read public project metadata only. The native host independently checks the
  // observed URL and expected game/project before accepting this result.
  const result = { url: "", canonicalUrl: null, gameDomain: null, modId: null, title: null, iconUrl: null, state: "pending" };
  const normalizeText = (value) => value.replace(/\s+/g, " ").trim();
  const positiveId = (value) => /^[0-9]{1,20}$/.test(value) && BigInt(value) > 0n
    && BigInt(value) <= 18446744073709551615n ? BigInt(value).toString() : null;
  const projectUrl = (value) => {
    if (typeof value !== "string" || !value || value.length > 2048 || /[\x00-\x20\\]/.test(value)) return null;
    let parsed;
    try { parsed = new URL(value, location.href); } catch { return null; }
    if (parsed.protocol !== "https:" || parsed.username || parsed.password || parsed.port
      || !["nexusmods.com", "www.nexusmods.com"].includes(parsed.hostname)) return null;
    const match = parsed.pathname.match(/^\/(?:games\/)?([a-z0-9_-]{1,128})\/mods\/([0-9]{1,20})(?:\/files(?:\/([0-9]{1,20}))?)?\/?$/);
    if (!match) return null;
    const modId = positiveId(match[2]);
    if (!modId || (match[3] && !positiveId(match[3]))) return null;
    // Query strings may contain signed-download or verification information.
    return { url: parsed.origin + parsed.pathname, gameDomain: match[1], modId };
  };
  const current = projectUrl(location.href);
  if (!current) return { ...result, state: "invalid" };
  result.url = current.url;
  const sameProject = (other) => other && other.gameDomain === current.gameDomain && other.modId === current.modId;

  if (/^(?:just a moment|attention required|security verification|verify (?:you are|you're) human|请稍候)/i.test(document.title.slice(0, 240))
    || document.querySelector('#challenge-running, #challenge-stage, #challenge-form, #cf-chl-widget, .cf-error-details')) {
    return { ...result, state: "challenge" };
  }

  const canonicals = document.head.querySelectorAll('link[rel~="canonical"]');
  if (canonicals.length > 4) return { ...result, state: "invalid" };
  for (const link of canonicals) {
    const canonical = projectUrl(link.getAttribute("href"));
    if (!sameProject(canonical)) return { ...result, state: "invalid" };
    result.canonicalUrl = canonical.url;
  }
  const socialUrls = document.head.querySelectorAll('meta[property="og:url"]');
  if (socialUrls.length > 4) return { ...result, state: "invalid" };
  for (const meta of socialUrls) {
    if (!sameProject(projectUrl(meta.getAttribute("content")))) return { ...result, state: "invalid" };
  }
  // Nexus file pages can omit the HTML canonical tag. Resolve their identity
  // from the validated current project URL, while rejecting any conflicting
  // canonical/OG URL above. Keep query strings out of the native metadata.
  result.canonicalUrl ??= current.url;

  const socialTitles = document.head.querySelectorAll('meta[property="og:title"]');
  if (socialTitles.length > 4) return { ...result, state: "invalid" };
  const rawSocialTitles = new Set();
  for (const meta of socialTitles) {
    const value = meta.getAttribute("content") ?? "";
    if (value.length > 1024) return { ...result, state: "invalid" };
    const title = normalizeText(value);
    if (title) rawSocialTitles.add(title);
  }

  // Nexus also renders login/navigation headings. Prefer the project heading;
  // never let headings inside an author's description identify the project.
  const excluded = 'nav, aside, dialog, [role="dialog"], [aria-modal="true"], [hidden], [aria-hidden="true"], #description, #mod_description, .mod_description, .mod-description, .wysiwyg';
  const scoped = document.querySelectorAll("#pagetitle h1, main h1");
  const headings = scoped.length ? scoped : document.querySelectorAll("h1");
  if (headings.length > 16) return { ...result, state: "invalid" };
  const headingTitles = new Set();
  for (const heading of headings) {
    if (heading.closest(excluded)) continue;
    if (heading.textContent.length > 1024) return { ...result, state: "invalid" };
    const title = normalizeText(heading.textContent);
    if (title) headingTitles.add(title);
  }
  const siteSuffix = /^\s+(?:at\s+.+\s+Nexus(?:\s*[-–—|]\s*Mods and community)?|(?:at|[-–—|])\s*Nexus Mods(?:\s*[-–—|]\s*Mods and community)?)$/i;
  const titles = new Set();
  for (const raw of rawSocialTitles) {
    if (headingTitles.size) {
      // The full project heading owns the boundary: "Server at Home" must not
      // become "Server" when OG appends "at 7 Days to Die Nexus ...".
      const matching = [...headingTitles].filter((heading) => raw === heading
        || (raw.startsWith(heading) && siteSuffix.test(raw.slice(heading.length))));
      if (matching.length !== 1) return { ...result, state: "invalid" };
      titles.add(matching[0]);
    } else {
      // Without a heading, require the complete legacy site marker and retain
      // all text before its last "at" separator, including "at" inside names.
      titles.add(raw
        .replace(/\s+(?:at|[-–—|])\s*Nexus Mods(?:\s*[-–—|]\s*Mods and community)?$/i, "")
        .replace(/^(.+)\s+at\s+.+\s+Nexus\s*[-–—|]\s*Mods and community$/i, "$1").trim());
    }
  }
  if (titles.size > 1) return { ...result, state: "invalid" };
  const socialTitle = titles.values().next().value ?? null;
  if (!socialTitle && headingTitles.size > 1) return { ...result, state: "invalid" };
  const title = socialTitle ?? headingTitles.values().next().value ?? null;
  if (!title || /^(?:Nexus Mods|Please log in|Log in|Sign in|Access denied|Page not found|Not found|404(?:\s.*)?)$/i.test(title)) return result;
  result.title = title.slice(0, 240);

  const images = document.head.querySelectorAll('meta[property="og:image"], meta[property="og:image:secure_url"]');
  if (images.length <= 8) {
    const icons = new Set();
    for (const meta of images) {
      const value = meta.getAttribute("content");
      if (!value || value.length > 2048) continue;
      let url;
      try { url = new URL(value, location.href); } catch { continue; }
      if (url.protocol === "https:" && url.hostname === "staticdelivery.nexusmods.com"
        && !url.username && !url.password && !url.port && !url.search && !url.hash
        && /^\/mods\/[1-9][0-9]*\/images\//.test(url.pathname) && /\.(?:png|jpe?g|webp|gif)$/i.test(url.pathname)) {
        icons.add(url.href);
      }
    }
    if (icons.size === 1) result.iconUrl = icons.values().next().value;
  }
  return { ...result, gameDomain: current.gameDomain, modId: current.modId, state: "ready" };
})();
