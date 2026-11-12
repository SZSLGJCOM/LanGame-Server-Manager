((target) => {
  // Bind the site's ordinary Manual action or quarantine notice to the
  // verified project and file row before using its download state.
  const id = (value) => {
    if (typeof value === "number") {
      if (!Number.isSafeInteger(value)) return null;
      value = String(value);
    }
    return typeof value === "string" && /^[0-9]{1,20}$/.test(value)
      && BigInt(value) > 0n && BigInt(value) <= 18446744073709551615n ? BigInt(value).toString() : null;
  };
  if (location.protocol !== "https:" || location.username || location.password || location.port
    || !["nexusmods.com", "www.nexusmods.com"].includes(location.hostname)) return [];
  const match = location.pathname.match(/^\/(?:games\/)?([a-z0-9_-]+)\/mods\/([0-9]+)(?:\/files(?:\/[0-9]+)?)?\/?$/);
  if (!match || match[1] !== target.gameDomain || id(match[2]) !== target.modId) return [];
  const roots = document.querySelectorAll("#mod_files");
  if (roots.length !== 1) return [];
  const excluded = 'nav,aside,footer,article,template,dialog,[role="dialog"],[aria-modal="true"],#description,#mod_description,.mod_description,.mod-description,.wysiwyg';
  const shown = (element, row) => {
    for (let parent = element; parent; parent = parent.parentElement) {
      const style = getComputedStyle(parent);
      if (parent.matches(excluded + ',[hidden],[aria-hidden="true"]')
        || (style.display === "none" && parent !== row) || style.visibility === "hidden" || style.opacity === "0") return false;
    }
    return true;
  };
  const rowIdentity = (element) => {
    const row = element.closest("dd[data-id]"), header = row?.previousElementSibling;
    const fileId = id(row?.getAttribute("data-id"));
    if (!fileId || header?.tagName !== "DT" || id(header.getAttribute("data-id")) !== fileId
      || !row.parentElement?.matches("dl.accordion") || !shown(element, row)) return null;
    return { fileId, isMain: !!element.closest("#file-container-main-files"),
      isMiscellaneous: !!element.closest("#file-container-miscellaneous-files") };
  };
  const controls = [];
  // Legacy rows retain the public filename on their matching DT header.
  // The selector joins this hint to the same anchor's verified project/file URL.
  for (const element of roots[0].querySelectorAll(".accordion-downloads a[href]")) {
    const identity = rowIdentity(element);
    if (!identity) continue;
    const name = element.closest("dd[data-id]").previousElementSibling.getAttribute("data-name");
    controls.push({ element, ...identity, legacy: true,
      name: typeof name === "string" && name.length <= 256 ? name.replace(/\s+/g, " ").trim() : null });
  }
  for (const element of roots[0].querySelectorAll('.accordion-downloads > li > mod-download-modal[placement="files_tab"]')) {
    const identity = rowIdentity(element);
    if (!identity) continue;
    const raw = element.getAttribute("file");
    if (!raw || raw.length > 16384) continue;
    let file, destination;
    try {
      file = JSON.parse(raw);
      if (!file || typeof file !== "object" || Array.isArray(file) || typeof file.downloadUrl !== "string") continue;
      destination = new URL(file.downloadUrl, location.href);
    } catch { continue; }
    const fileUid = id(file.uid);
    if (!fileUid || destination.origin !== location.origin || destination.username || destination.password || destination.port
      || destination.pathname !== `/api/files/${fileUid}/download` || destination.search || destination.hash) continue;
    // This name belongs to the same bound file payload as its UID/download URL.
    // Do not infer a filename from author prose, page headings or requirements.
    const name = typeof file.name === "string" && file.name.length <= 256
      ? file.name.replace(/\s+/g, " ").trim() : null;
    controls.push({ element, ...identity, fileUid, name });
  }
  for (const element of roots[0].querySelectorAll('.accordion-downloads > li.quarantine-notice')) {
    const identity = rowIdentity(element);
    const notice = element.querySelector(":scope > p")?.textContent.replace(/\s+/g, " ").trim();
    if (identity && /^This file has been quarantined by our automated safety checks\./i.test(notice ?? "")) {
      controls.push({ element, ...identity, quarantined: true });
    }
  }
  return controls;
})
