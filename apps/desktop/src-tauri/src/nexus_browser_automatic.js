((target, page, rendered, authorizedSelection = null, fileControls = []) => {
  // Use ordinary website controls. Do not fetch hidden endpoints, extract
  // cookies, skip the site's countdown, or interact with verification widgets.
  const result = (state, reason, phase) => ({ state, ...(reason ? { reason } : {}), ...(phase ? { phase } : {}) });
  const working = (phase = "selecting") => result("working", undefined, phase);
  const downloadStartWaitMs = 90000;
  const id = (value) => typeof value === "string" && /^[0-9]{1,20}$/.test(value)
    && BigInt(value) > 0n && BigInt(value) <= 18446744073709551615n ? BigInt(value).toString() : null;
  const project = (value) => {
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
  if (!document.body || document.readyState === "loading" || !rendered.hasContent) return result("loading");
  if (location.protocol === "https:" && location.hostname === "users.nexusmods.com") return result("blocked", "login");
  const current = project(location.href);
  if (!current || page.state === "invalid") return result("blocked", "project_identity");
  if (target.fileId && current.fileId && target.fileId !== current.fileId) return result("blocked", "file_selection");

  const now = performance.now(), key = Symbol.for("langame.nexus.website-download");
  const scope = `${target.gameDomain}/${target.modId}`;
  let memory = document[key];
  if (!memory || memory.scope !== scope) {
    memory = document[key] = { scope, stage: current.fileId, since: now, actions: new Map() };
  }
  // The site may finish its own verification without user interaction. Keep
  // that wait bounded and report it; never expose or click a challenge widget.
  if (page.state === "challenge") {
    memory.verificationSince ??= now;
    return now - memory.verificationSince < 30000 ? working("verifying") : result("blocked", "verification");
  }
  if (memory.verificationSince !== undefined) { delete memory.verificationSince; memory.since = now; }
  if (memory.stage !== current.fileId) { memory.stage = current.fileId; memory.since = now; }
  const waitForControls = () => now - memory.since < 8000 ? working() : result("blocked", "website_controls");
  // The metadata extractor resolves this project reference from the current
  // URL when the optional HTML canonical tag is absent.
  if (!page.canonicalUrl || !project(page.canonicalUrl)) return waitForControls();

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
    && Array.from(element.getClientRects()).some((rect) => rect.width > 0 && rect.height > 0);
  const disabled = (element) => element.matches(':disabled, [aria-disabled="true"]');
  const label = (element) => (element.getAttribute("aria-label")
    || (element.matches("input") ? element.value : element.innerText || element.textContent) || "")
    .replace(/\s+/g, " ").trim().replace(/^(?:download|file_download)\s+(?=(?:manual|slow)\s+download$)/i, "");
  // The current website renders Slow download in an open shadow root. Only
  // enter its official component for this selected file and game, not arbitrary
  // shadow trees or author content; use its normal button and countdown.
  const components = current.fileId ? [...document.querySelectorAll("mod-file-download")].filter((element) =>
    usable(element) && shown(element) && id(element.getAttribute("file-id")) === current.fileId
    && [target.gameDomain, `/${target.gameDomain}`].includes(element.getAttribute("game-domain"))) : [];
  const roots = [document, ...components.flatMap((element) => element.shadowRoot ? [element.shadowRoot] : []),
    ...fileControls.flatMap(({ element }) => element.shadowRoot ? [element.shadowRoot] : [])];
  const portal = document.getElementById("next-shadow-root");
  if ((components.length || fileControls.some((control) => !control.quarantined && !control.legacy))
    && portal?.shadowRoot && usable(portal) && shown(portal)) roots.push(portal.shadowRoot);
  const query = (selector) => roots.flatMap((root) => [...root.querySelectorAll(selector)]);
  if (components.some((element) => element.getAttribute("user-is-logged-in") === "false")) return result("blocked", "login");
  // Nexus can put its download login gate on the file list, before a file_id
  // exists, or in the selected file's download table. Match the visible notice
  // itself; header sign-in links and author content do not prove a download gate.
  const texts = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  while (texts.nextNode()) {
    const element = texts.currentNode.parentElement;
    if (!element || !usable(element) || element.closest('script,style,template,textarea,select')) continue;
    const content = texts.currentNode.textContent.replace(/\s+/g, " ").trim();
    if (!/^You (?:must|have to) be logged in\b/i.test(content)) continue;
    if (!current.fileId && !/^You (?:must|have to) be logged in to download files\b/i.test(content)) continue;
    if (displayed(element) && /^You (?:must|have to) be logged in(?: to download files)?[.!]?(?:\s+Log in here\s*[.!]?)?$/i
      .test((element.innerText ?? "").replace(/\s+/g, " ").trim())) return result("blocked", "login");
  }
  const activate = (element, action, phase) => {
    const previous = memory.actions.get(action);
    if (previous !== undefined) return now - previous < downloadStartWaitMs ? working(phase) : result("blocked", "download_not_started");
    if (disabled(element)) return waitForControls();
    memory.actions.set(action, now);
    memory.since = now;
    // Collapsed file rows may keep their ordinary Manual link in the DOM.
    // The link's own site handler/navigation still performs the download flow.
    if (displayed(element)) element.scrollIntoView({ block: "center" });
    element.click();
    return working(phase);
  };
  const sameSelection = (left, right) => left && right
    && left.fileId === right.fileId && left.fileUid === right.fileUid;
  const fileLink = (element, selection) => {
    if (!element.matches("a[href]")) return false;
    let destination;
    try { destination = new URL(element.getAttribute("href"), location.href); } catch { return false; }
    return destination.origin === location.origin && !destination.username && !destination.password && !destination.port
      && destination.pathname === `/api/files/${selection.fileUid}/download` && !destination.search && !destination.hash;
  };
  const dialogs = query('dialog[open], [role="dialog"], [aria-modal="true"]')
    .filter((element) => usable(element) && displayed(element));
  if (dialogs.length) {
    // The site's new Manual button can first show this file's requirements.
    // Continue only its bound Manual link; dependency downloads and other
    // dialogs are not the file selected by this operation.
    const selection = memory.fileSelection;
    if (dialogs.length === 1 && sameSelection(selection, authorizedSelection)
      && fileControls.some((control) => sameSelection(control, selection))
      && [...dialogs[0].querySelectorAll("h1,h2,h3")].some((heading) => label(heading) === "Download mod file")) {
      const manual = [...dialogs[0].querySelectorAll("a[href]")].filter((element) =>
        usable(element) && displayed(element) && /^manual download$/i.test(label(element)) && fileLink(element, selection));
      if (manual.length === 1) return activate(manual[0], `modal-manual:${selection.fileId}`, "selecting");
    }
    return result("blocked", "website_dialog");
  }

  if (current.fileId) {
    const slow = query('button, a[href], input[type="button"], input[type="submit"]')
      .filter((element) => usable(element) && displayed(element) && /^slow download$/i.test(label(element))
        && (!element.matches("a") || project(element.getAttribute("href"))?.fileId === current.fileId));
    if (slow.length > 1) return result("blocked", "file_selection");
    if (slow.length === 1) return activate(slow[0], `slow:${current.fileId}`, "waiting");
    // A countdown string also appears on logged-out pages. Only the native
    // DownloadEvent proves a download; never manufacture a download URL here.
    const clicked = memory.actions.get(`slow:${current.fileId}`);
    if (clicked !== undefined) return now - clicked < downloadStartWaitMs ? working("waiting") : result("blocked", "download_not_started");
  }

  const headings = [...document.querySelectorAll("h1,h2,h3,h4,h5,h6")].filter(usable);
  const categoryLabel = (heading) => label(heading).replace(/\s*\([0-9]+\)$/, "").toLowerCase();
  const groups = headings.filter((heading) => categoryLabel(heading) === "main files"
    || (categoryLabel(heading) === "miscellaneous files" && heading.closest("#mod_files") && !heading.closest("dt,dd"))).map((heading) => {
    const level = Number(heading.tagName[1]);
    const end = headings.find((other) => Number(other.tagName[1]) <= level
      && Boolean(heading.compareDocumentPosition(other) & Node.DOCUMENT_POSITION_FOLLOWING));
    return { heading, end, kind: categoryLabel(heading) };
  });
  const inGroup = (element, category) => groups.some(({ heading, end, kind }) => kind === category
    && Boolean(heading.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_FOLLOWING)
    && (!end || Boolean(element.compareDocumentPosition(end) & Node.DOCUMENT_POSITION_FOLLOWING)));
  const isMiscellaneous = (control) => !control.element.closest(
    "#file-container-optional-files,#file-container-old-files,#file-container-archived-files",
  ) && (control.isMiscellaneous || inGroup(control.element, "miscellaneous files"));
  const specified = target.fileId ?? current.fileId;
  let files = new Map();
  const miscellaneous = new Map();
  for (const link of document.querySelectorAll("a[href]")) {
    if (!usable(link) || !/^manual download$/i.test(label(link))) continue;
    const destination = project(link.getAttribute("href"));
    const bound = fileControls.find((control) => control.legacy && control.element === link && control.fileId === destination?.fileId);
    if (!destination?.fileId || (specified && destination.fileId !== specified)) continue;
    const choices = specified || (bound ? bound.isMain : inGroup(link, "main files")) ? files
      : bound && isMiscellaneous(bound) ? miscellaneous : null;
    if (!choices) continue;
    // Multiple presentations of the same file (for example a collapsed row)
    // are one choice. Prefer its visible, enabled control when present.
    const prior = choices.get(destination.fileId);
    if (!prior || (displayed(link) && !disabled(link))) choices.set(destination.fileId, { element: link, name: bound?.name, boundName: Boolean(bound) });
  }
  for (const control of fileControls) {
    if (control.legacy) continue;
    if (specified && control.fileId !== specified) continue;
    const choices = specified || control.isMain ? files
      : isMiscellaneous(control) ? miscellaneous : null;
    if (!choices) continue;
    if (control.quarantined) {
      choices.set(control.fileId, { quarantined: true });
      continue;
    }
    const selection = { fileId: control.fileId, fileUid: control.fileUid };
    const manual = control.element.shadowRoot ? [...control.element.shadowRoot.querySelectorAll('button,a[href]')]
      .filter((element) => usable(element) && displayed(element) && /^manual download$/i.test(label(element))
        && (!element.matches("a") || fileLink(element, selection))) : [];
    const prior = choices.get(control.fileId);
    if (prior?.selection && !sameSelection(prior.selection, selection)) {
      choices.set(control.fileId, { ...prior, ambiguous: true });
    } else if (prior?.element && displayed(prior.element)) {
      // The site can render its legacy link beside the modern component.
      // Keep that usable control and enrich it with the same row's bound UID/name.
      choices.set(control.fileId, { ...prior, selection, name: control.name ?? prior.name });
    } else {
      choices.set(control.fileId, { element: manual.length === 1 ? manual[0] : null,
        selection, name: control.name, ambiguous: Boolean(prior?.ambiguous) || manual.length > 1 });
    }
  }
  // Some projects publish their sole primary download under Miscellaneous.
  // Main files, including unavailable or unhydrated ones, retain precedence.
  // Optional and old files never become this fallback.
  const mainPending = groups.some(({ heading, kind }) => kind === "main files" && !heading.closest("dt,dd"));
  if (!specified && !files.size && !mainPending) files = miscellaneous;
  const words = (choice) => choice.name.replace(/([a-z])([A-Z])/g, "$1 $2").toLowerCase().split(/[^a-z0-9]+/);
  // Only names bound to this site's verified file row provide these hints.
  const serverFile = (choice) => {
    if ((!choice.selection && !choice.boundName) || typeof choice.name !== "string" || /client/i.test(choice.name)) return false;
    const tokens = words(choice);
    return tokens.includes("server") && !tokens.some((word) =>
      ["both", "no", "not", "non", "without", "except", "exclude", "excluding", "unsupported", "incompatible", "disabled", "disable", "off", "less"].includes(word));
  };
  const linuxOnly = (choice) => serverFile(choice) && words(choice).includes("linux") && !words(choice).includes("windows");
  const windowsOnly = (choice) => serverFile(choice) && words(choice).includes("windows") && !words(choice).includes("linux");
  let selected = files.size === 1 ? files.entries().next().value : null;
  if (files.size > 1) {
    // Mixed client/server and negated labels remain a choice for the user;
    // ordering, version and numeric IDs never decide between files.
    let servers = [...files].filter(([, choice]) => serverFile(choice));
    // These downloads run on the local Windows server. Distinguish an explicit
    // Windows/Linux split, while unknown platforms and multiple Windows variants
    // keep their ambiguity. A caller-specified file still has precedence.
    const windows = servers.filter(([, choice]) => windowsOnly(choice));
    if (windows.length === 1 && servers.every(([, choice]) => windowsOnly(choice) || linuxOnly(choice))) servers = windows;
    // A quarantine row has no downloadable file payload/name. Keep its existing
    // ambiguity rather than silently treating an unknown variant as a client.
    if (specified || [...files.values()].some((choice) => choice.quarantined) || servers.length !== 1) {
      return result("blocked", "file_selection");
    }
    selected = servers[0];
  }
  if (selected && !specified && linuxOnly(selected[1])) return result("blocked", "file_selection");
  if (selected) {
    const [fileId, choice] = selected;
    if (choice.quarantined) return result("blocked", "file_quarantined");
    if (choice.ambiguous) return result("blocked", "file_selection");
    if (choice.selection) {
      if (memory.fileSelection && !sameSelection(memory.fileSelection, choice.selection)) return result("blocked", "file_selection");
      // Let native bind this public navigation to the verified project before
      // a normal site click can synchronously start navigating.
      if (!sameSelection(authorizedSelection, choice.selection)) return { ...working(), fileSelection: choice.selection };
      memory.fileSelection = choice.selection;
    }
    return choice.element ? activate(choice.element, `manual:${fileId}`, "selecting") : waitForControls();
  }
  return waitForControls();
})
