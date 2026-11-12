((target, fileControls = []) => {
  // Only fixed categories, counts and public numeric identities leave the page.
  // Do not return labels, HTML, URL parameters, cookies or form values.
  const result = {
    readyState: document.readyState, fileId: null, canonicalCount: 0, mainHeadingCount: 0,
    manualControlCount: 0, matchingManualCount: 0, slowControlCount: 0, matchingSlowCount: 0,
    loginNoticeCount: 0, visibleDialogCount: 0, challengeFramePresent: false, controls: [],
  };
  const id = (value) => typeof value === "string" && /^[0-9]{1,20}$/.test(value)
    && BigInt(value) > 0n && BigInt(value) <= 18446744073709551615n ? BigInt(value).toString() : null;
  const project = (value) => {
    if (typeof value !== "string") return null;
    let url;
    try { url = new URL(value, location.href); } catch { return null; }
    if (url.protocol !== "https:" || url.username || url.password || url.port
      || !["nexusmods.com", "www.nexusmods.com"].includes(url.hostname)) return null;
    const match = url.pathname.match(/^\/(?:games\/)?([a-z0-9_-]+)\/mods\/([0-9]+)(?:\/files(?:\/([0-9]+))?)?\/?$/);
    if (!match || match[1] !== target.gameDomain || id(match[2]) !== target.modId) return null;
    const query = url.searchParams.getAll("file_id");
    if (query.length > 1 || (query.length && !id(query[0])) || (match[3] && !id(match[3]))) return null;
    const pathFile = match[3] ? id(match[3]) : null, queryFile = query.length ? id(query[0]) : null;
    if (pathFile && queryFile && pathFile !== queryFile) return null;
    return { fileId: pathFile ?? queryFile };
  };
  const current = project(location.href);
  if (!current || !document.body) return result;
  result.fileId = current.fileId;
  const excluded = 'nav, aside, footer, #description, #mod_description, .mod_description, .mod-description, .wysiwyg';
  const parentOf = (element) => element.parentElement ?? element.getRootNode().host ?? null;
  const usable = (element) => {
    for (let parent = element; parent; parent = parentOf(parent)) if (parent.matches(excluded)) return false;
    return true;
  };
  const shown = (element) => {
    for (let parent = element; parent; parent = parentOf(parent)) {
      if (parent.matches('[hidden], [aria-hidden="true"]')) return false;
      const style = getComputedStyle(parent);
      if (style.display === "none" || style.visibility === "hidden" || style.opacity === "0") return false;
    }
    return true;
  };
  const displayed = (element) => shown(element)
    && [...element.getClientRects()].some((rect) => rect.width > 0 && rect.height > 0);
  // Mirror the production selector's bound download component and portal.
  const components = current.fileId ? [...document.querySelectorAll("mod-file-download")].filter((element) =>
    usable(element) && shown(element) && id(element.getAttribute("file-id")) === current.fileId
    && [target.gameDomain, `/${target.gameDomain}`].includes(element.getAttribute("game-domain"))) : [];
  const roots = [document, ...components.flatMap((element) => element.shadowRoot ? [element.shadowRoot] : []),
    ...fileControls.flatMap(({ element }) => element.shadowRoot ? [element.shadowRoot] : [])];
  const portal = document.getElementById("next-shadow-root");
  if ((components.length || fileControls.some((control) => !control.quarantined && !control.legacy))
    && portal?.shadowRoot && usable(portal) && shown(portal)) roots.push(portal.shadowRoot);
  const query = (selector) => roots.flatMap((root) => [...root.querySelectorAll(selector)]);
  const text = (value) => (value ?? "").replace(/\s+/g, " ").trim();
  const label = (element) => text(element.getAttribute("aria-label") || element.innerText || element.textContent)
    .replace(/^(?:download|file_download)\s+(?=(?:manual|slow)\s+download$)/i, "");
  const kind = (label) => /^manual download$/i.test(label) ? "manual"
    : /^slow download$/i.test(label) ? "slow" : /download/i.test(label) ? "download"
      : /^(?:log in|sign in)(?: here)?[.!]?$/i.test(label) ? "login" : null;
  const controls = [];
  for (const element of query('a[href],button,input[type="button"],input[type="submit"]')) {
    if (!usable(element)) continue;
    const category = kind(label(element));
    if (!category) continue;
    let destination = element.matches("a") ? project(element.getAttribute("href")) : null;
    if (!destination && category === "manual") {
      const owner = fileControls.find((control) => {
        if (control.legacy) return false;
        if (!element.matches("a")) return control.element.shadowRoot?.contains(element);
        let url;
        try { url = new URL(element.getAttribute("href"), location.href); } catch { return false; }
        return url.origin === location.origin && !url.username && !url.password && !url.port
          && url.pathname === `/api/files/${control.fileUid}/download` && !url.search && !url.hash;
      });
      if (owner) destination = { fileId: owner.fileId };
    }
    controls.push({ kind: category, tag: element.tagName.toLowerCase(), displayed: displayed(element),
      disabled: element.matches(':disabled,[aria-disabled="true"]'),
      matchesProject: destination !== null, fileId: destination?.fileId ?? null });
  }
  const manual = controls.filter((control) => control.kind === "manual");
  const slow = controls.filter((control) => control.kind === "slow");
  result.canonicalCount = document.head.querySelectorAll('link[rel~="canonical"]').length;
  result.mainHeadingCount = [...document.querySelectorAll("h1,h2,h3,h4,h5,h6")]
    .filter((element) => usable(element) && /^main files$/i.test(label(element))).length;
  result.manualControlCount = manual.length;
  result.matchingManualCount = manual.filter((control) => control.matchesProject && control.fileId).length;
  result.slowControlCount = slow.length;
  result.matchingSlowCount = current.fileId ? slow.filter((control) => control.displayed
    && (control.tag !== "a" || (control.matchesProject && control.fileId === current.fileId))).length : 0;
  const notices = new Set(), texts = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  while (texts.nextNode()) {
    const element = texts.currentNode.parentElement;
    if (!element || !usable(element) || element.closest('script,style,template,textarea,select')) continue;
    if (/(?:(?:must|have to) be logged in|log in here|sign in to download)/i.test(text(texts.currentNode.textContent))
      && displayed(element) && /(?:(?:must|have to) be logged in|log in here|sign in to download)/i.test(text(element.innerText))) notices.add(element);
  }
  result.loginNoticeCount = notices.size + components.filter((element) => element.getAttribute("user-is-logged-in") === "false").length;
  result.visibleDialogCount = query('dialog[open],[role="dialog"],[aria-modal="true"]')
    .filter((element) => usable(element) && displayed(element)).length;
  result.challengeFramePresent = [...document.querySelectorAll('iframe[src]')].some((frame) => {
    if (!usable(frame) || !displayed(frame)) return false;
    try { return new URL(frame.getAttribute("src"), location.href).hostname === "challenges.cloudflare.com"; } catch { return false; }
  });
  const priority = { manual: 0, slow: 1, download: 2, login: 3 };
  result.controls = controls.sort((left, right) => priority[left.kind] - priority[right.kind]).slice(0, 8);
  return result;
})
