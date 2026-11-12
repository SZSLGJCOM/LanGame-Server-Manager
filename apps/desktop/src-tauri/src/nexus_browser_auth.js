(() => {
  // These fixed website structures expose state, not account data.
  // A missing signal (login/challenge/loading) must not invent a signed-out state.
  if (location.protocol !== "https:" || location.username || location.password || location.port
    || !["nexusmods.com", "www.nexusmods.com", "users.nexusmods.com"].includes(location.hostname)
    || document.readyState === "loading") return null;
  const excluded = '#description, #mod_description, .mod_description, .mod-description, .wysiwyg, article, template';
  if (location.hostname === "users.nexusmods.com") {
    // An existing session can land here instead of returning to WWW. This is
    // the account navigation observed on the official security page, not an
    // arbitrary sign-out link or the presence of a login form.
    if (location.pathname !== "/account/security") return null;
    for (const element of document.querySelectorAll(
      '#main-content-wrapper > .top-half > .jumbotron.jumbotron-border-bottom > .links > .right-link > a[href]',
    )) {
      if (element.closest(excluded)) continue;
      let destination;
      try { destination = new URL(element.getAttribute("href"), location.href); } catch { continue; }
      if (destination.origin !== "https://users.nexusmods.com" || destination.username || destination.password
        || destination.pathname !== "/auth/sign_out" || destination.search || destination.hash) continue;
      let shown = true;
      for (let parent = element; parent; parent = parent.parentElement) {
        const style = getComputedStyle(parent);
        if (parent.hidden || parent.getAttribute("aria-hidden") === "true"
          || style.display === "none" || style.visibility !== "visible" || style.opacity === "0") {
          shown = false;
          break;
        }
      }
      if (shown && [...element.getClientRects()].some(rect => rect.width > 0 && rect.height > 0)) return "signed_in";
    }
    return null;
  }
  const values = new Set();
  for (const element of document.querySelectorAll(
    '.rj-header-wrap .quick-search-holder > quick-search[is-logged-in], '
    + '.rj-user-nav-right .mobile-search-container > mobile-quick-search[is-logged-in]',
  )) {
    if (element.closest(excluded)) continue;
    const value = element.getAttribute("is-logged-in");
    if (value === "true" || value === "false") values.add(value);
  }
  if (values.size > 1) return "unknown";
  if (values.has("true")) return "signed_in";
  if (values.has("false")) return "signed_out";
  return null;
})()
