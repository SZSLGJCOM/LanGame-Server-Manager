function normalizeSteamAssetUrl(value: string) {
  if (value.startsWith("//")) {
    return `https:${value}`;
  }
  return value;
}

function unwrapSteamNode(element: Element) {
  const parent = element.parentNode;
  if (!parent) {
    return;
  }

  while (element.firstChild) {
    parent.insertBefore(element.firstChild, element);
  }

  parent.removeChild(element);
}

export function sanitizeSteamAboutHtml(rawHtml: string) {
  if (!rawHtml.trim()) {
    return "";
  }

  const parser = new DOMParser();
  const document = parser.parseFromString(rawHtml, "text/html");
  const allowedTags = new Set(["a", "b", "blockquote", "br", "div", "em", "h1", "h2", "h3", "h4", "hr", "i", "img", "li", "ol", "p", "source", "span", "strong", "u", "ul", "video"]);
  const steamLayoutAttributes = new Set(["style"]);

  for (const blocked of document.body.querySelectorAll("script, iframe, object, embed")) {
    blocked.remove();
  }

  const elements = Array.from(document.body.querySelectorAll("*"));
  for (const element of elements) {
    const tagName = element.tagName.toLowerCase();
    if (!allowedTags.has(tagName)) {
      unwrapSteamNode(element);
      continue;
    }

    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value.trim();

      if (name.startsWith("on")) {
        element.removeAttribute(attribute.name);
        continue;
      }

      if (name === "href" || name === "src" || name === "poster") {
        const normalized = normalizeSteamAssetUrl(value);
        if (!/^https?:/i.test(normalized)) {
          element.removeAttribute(attribute.name);
          continue;
        }
        element.setAttribute(attribute.name, normalized);
        continue;
      }

      if (name === "width" || name === "height") {
        // Steam's display size is the aspect-ratio hint. CSS scales it down;
        // dropping it leaves videos at the 300x150 default and the card clips them.
        if ((tagName === "img" || tagName === "video") && /^[1-9]\d{0,4}$/.test(value)) {
          continue;
        }
        element.removeAttribute(attribute.name);
        continue;
      }

      if (steamLayoutAttributes.has(name)) {
        element.removeAttribute(attribute.name);
        continue;
      }

      if ((tagName === "source" && name === "type")
        || ["alt", "autoplay", "class", "controls", "loop", "muted", "playsinline", "title"].includes(name)) {
        continue;
      }

      element.removeAttribute(attribute.name);
    }

    if (tagName === "a") {
      element.setAttribute("target", "_blank");
      element.setAttribute("rel", "noreferrer");
    }
  }

  return document.body.innerHTML.trim();
}
