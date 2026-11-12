((target, page, rendered) => {
  // Diagnostic output is a bounded public control summary, never page HTML,
  // input values, cookies, signed links, or Cloudflare challenge parameters.
  const text = (value, max = 180) => String(value ?? "").replace(/https?:\/\/\S+/gi, "[url]")
    .replace(/[A-Za-z0-9_=-]{48,}/g, "[opaque]").replace(/\s+/g, " ").trim().slice(0, max);
  const safeHref = (value) => {
    let url;
    try { url = new URL(value, location.href); } catch { return null; }
    if (url.protocol !== "https:" || url.username || url.password || url.port
      || !["nexusmods.com", "www.nexusmods.com"].includes(url.hostname)) return null;
    const match = url.pathname.match(/^\/(?:games\/)?([a-z0-9_-]{1,128})\/mods\/([0-9]{1,20})(?:\/files(?:\/([0-9]{1,20}))?)?\/?$/);
    if (!match || match[1] !== target.gameDomain || match[2] !== target.modId) return null;
    const safe = new URL(url.origin + url.pathname);
    const file = url.searchParams.get("file_id");
    if (file && /^[0-9]{1,20}$/.test(file)) safe.searchParams.set("file_id", file);
    const tab = url.searchParams.get("tab");
    if (["files", "description"].includes(tab)) safe.searchParams.set("tab", tab);
    return safe.href;
  };
  const displayed = (element) => {
    if (element.closest('[hidden], [aria-hidden="true"]')) return false;
    for (let parent = element; parent; parent = parent.parentElement) {
      const style = getComputedStyle(parent);
      if (style.display === "none" || style.visibility === "hidden" || style.opacity === "0") return false;
    }
    return Array.from(element.getClientRects()).some((rect) => rect.width > 0 && rect.height > 0);
  };
  const inViewport = (element) => Array.from(element.getClientRects()).some((rect) =>
    rect.width > 0 && rect.height > 0 && rect.right > 0 && rect.bottom > 0
    && rect.left < innerWidth && rect.top < innerHeight);
  const label = (element) => text(element.getAttribute("aria-label")
    || (element.matches("input") ? "" : element.innerText || element.textContent));
  const relevant = (value) => /download|manual|slow|file|log ?in|sign ?in|verif|continue|accept|agree|require|security|cookie|select/i.test(value);
  const controls = [...document.querySelectorAll('a[href],button,[role="button"],input[type="submit"],input[type="button"]')];
  const headings = [...document.querySelectorAll("h1,h2,h3,h4,h5,h6")];
  const describe = (element) => ({
    tag: element.tagName.toLowerCase(), label: label(element), displayed: displayed(element),
    innerText: text(element.innerText), textContent: text(element.textContent),
    ariaLabel: text(element.getAttribute("aria-label")),
    inViewport: inViewport(element), disabled: element.matches(':disabled,[aria-disabled="true"]'),
    href: element.matches("a") ? safeHref(element.getAttribute("href")) : null,
    parentTag: element.parentElement?.tagName.toLowerCase() ?? null,
    excludedByAutomation: Boolean(element.closest('nav,aside,footer,#description,#mod_description,.mod_description,.mod-description,.wysiwyg')),
    previousHeadings: headings.filter((heading) => Boolean(heading.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_FOLLOWING))
      .slice(-3).map((heading) => ({ tag: heading.tagName.toLowerCase(), label: label(heading) })),
  });
  const canonicals = [...document.head.querySelectorAll('link[rel~="canonical"]')];
  const publicImage = (value) => {
    try {
      const url = new URL(value, location.href);
      return url.protocol === "https:" && url.hostname === "staticdelivery.nexusmods.com"
        && !url.username && !url.password && !url.port ? url.origin + url.pathname : null;
    } catch { return null; }
  };
  const icon = (() => {
    try {
      const value = new URL(page.iconUrl);
      return value.protocol === "https:" && value.hostname === "staticdelivery.nexusmods.com"
        ? value.origin + value.pathname : null;
    } catch { return null; }
  })();
  const selected = controls.filter((element) => relevant(label(element)) || relevant(element.textContent) || element.matches("button,[role=button]"));
  const publicId = (value) => typeof value === "string" && /^[1-9][0-9]{0,19}$/.test(value) ? value : null;
  const fileRows = [...document.querySelectorAll('#mod_files dd[data-id]')].slice(0, 32).map((row) => {
    const header = row.previousElementSibling, fileId = publicId(row.getAttribute('data-id'));
    const headerMatches = header?.tagName === 'DT' && publicId(header.getAttribute('data-id')) === fileId;
    const names = (file) => Object.fromEntries(['name','fileName','file_name','displayName'].map((key) =>
      [key, typeof file?.[key] === 'string' ? text(file[key], 256) : null]));
    return { fileId, main: Boolean(row.closest('#file-container-main-files')), headerMatches,
      dataName: headerMatches ? text(header.getAttribute('data-name'), 256) : null,
      headerName: headerMatches ? text(header.querySelector('.name')?.textContent, 256) : null,
      modern: [...row.querySelectorAll('mod-download-modal[placement="files_tab"]')].slice(0, 4).map((element) => {
        let file = null, endpoint = 'unavailable';
        const raw = element.getAttribute('file');
        try {
          if (raw && raw.length <= 16384) file = JSON.parse(raw);
          if (typeof file?.downloadUrl === 'string') {
            const url = new URL(file.downloadUrl, location.href);
            endpoint = url.origin !== location.origin ? 'other_origin'
              : url.pathname === `/api/files/${file.uid}/download` && !url.search && !url.hash ? 'bound_file_download' : 'other_same_origin';
          }
        } catch { file = null; }
        return { uid: publicId(String(file?.uid ?? '')), ...names(file), endpoint };
      }) };
  });
  return {
    readyState: document.readyState, url: safeHref(location.href), documentTitle: text(document.title, 240),
    metadata: { state: page.state, title: text(page.title, 240), iconUrl: icon,
      canonicalUrl: safeHref(page.canonicalUrl), canonicalMatchesTarget: Boolean(page.canonicalUrl && safeHref(page.canonicalUrl)) },
    hasContent: rendered.hasContent, viewport: { width: innerWidth, height: innerHeight },
    canonicalCount: canonicals.length,
    canonicals: canonicals.slice(0, 4).map((link) => ({ matchesTarget: Boolean(safeHref(link.getAttribute("href"))), href: safeHref(link.getAttribute("href")) })),
    socialUrls: [...document.head.querySelectorAll('meta[property="og:url"]')].slice(0, 4)
      .map((meta) => ({ matchesTarget: Boolean(safeHref(meta.getAttribute("content"))), href: safeHref(meta.getAttribute("content")) })),
    socialTitles: [...document.head.querySelectorAll('meta[property="og:title"]')].slice(0, 4).map((meta) => text(meta.getAttribute("content"), 240)),
    socialImages: [...document.head.querySelectorAll('meta[property="og:image"]')].slice(0, 4).map((meta) => publicImage(meta.getAttribute("content"))),
    projectHeadings: [...document.querySelectorAll('#pagetitle h1,main h1')].slice(0, 8)
      .map((element) => ({ tag: element.tagName.toLowerCase(), text: label(element), displayed: displayed(element),
        parentTag: element.parentElement?.tagName.toLowerCase() ?? null })),
    controlCount: controls.length, relevantControlCount: selected.length, fileRows,
    controls: selected.slice(0, 64).map(describe),
    headings: headings.slice(0, 32).map((element) => ({ tag: element.tagName.toLowerCase(), label: label(element), displayed: displayed(element) })),
    dialogs: [...document.querySelectorAll('dialog[open],[role="dialog"],[aria-modal="true"]')].filter(displayed).slice(0, 8)
      .map((element) => ({ tag: element.tagName.toLowerCase(), label: text(element.getAttribute("aria-label")),
        headings: [...element.querySelectorAll("h1,h2,h3")].slice(0, 4).map(label) })),
    loginNotices: [...document.querySelectorAll('div,span,section,td,h1,h2,h3,h4,h5,h6,p,[role="alert"]')].filter((element) => {
      const content = (element.textContent ?? "").replace(/\s+/g, " ").trim();
      return content.length <= 600 && /(?:(?:must|have to) be logged in|log in to (?:view|download)|sign in to download|registered)/i.test(content);
    }).slice(0, 16).map((element) => ({
      tag: element.tagName.toLowerCase(), id: text(element.id, 80), className: text(element.className, 120),
      displayed: displayed(element), inViewport: inViewport(element),
      excludedByAutomation: Boolean(element.closest('nav,aside,footer,#description,#mod_description,.mod_description,.mod-description,.wysiwyg')),
      innerText: text(element.innerText, 240), textContent: text(element.textContent, 240),
      ariaLabel: text(element.getAttribute("aria-label")),
      ancestors: [element.parentElement, element.parentElement?.parentElement].filter(Boolean)
        .map((parent) => ({ tag: parent.tagName.toLowerCase(), id: text(parent.id, 80), className: text(parent.className, 120) })),
    })),
  };
})
