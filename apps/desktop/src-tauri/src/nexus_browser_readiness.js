(() => {
  // The native URL owns the trust decision. Do not return query strings,
  // form values, cookies or other account data while checking visible content.
  const result = { url: location.origin + location.pathname, hasContent: false };
  if (!document.body || document.readyState === "loading") return result;
  const visible = (element) => {
    if (element.closest('[hidden], [aria-hidden="true"]')) return false;
    const style = getComputedStyle(element);
    return style.display !== "none" && style.visibility !== "hidden"
      && style.opacity !== "0"
      && Array.from(element.getClientRects()).some((rect) => rect.width > 0 && rect.height > 0
        && rect.right > 0 && rect.bottom > 0 && rect.left < innerWidth && rect.top < innerHeight);
  };
  // A head-only project response or an empty SPA shell must stay hidden.
  // Login fields and challenge frames can be usable before project metadata.
  const elements = document.body.querySelectorAll('h1, h2, p, button, a, input, select, iframe');
  result.hasContent = Array.from(elements).slice(0, 512).some((element) => visible(element)
    && (element.matches('input:not([type="hidden"]), select, iframe')
      || (element.innerText ?? "").trim().length >= 2));
  return result;
})();
