export type EmbeddedBrowserBounds = { x: number; y: number; width: number; height: number };

// Native children use physical client pixels, including the main WebView's zoom.
// Hide them when the DOM slot is clipped or covered so they cannot obscure UI.
export function visibleEmbeddedBrowserBounds(element: HTMLElement): EmbeddedBrowserBounds | null {
  const rect = element.getBoundingClientRect();
  if (document.hidden || rect.width < 2 || rect.height < 2 || rect.left < 0 || rect.top < 0
    || rect.right > innerWidth || rect.bottom > innerHeight) return null;
  for (const dialog of document.querySelectorAll('[role="dialog"][aria-modal="true"], [role="alertdialog"], dialog[open]')) {
    if (dialog.getClientRects().length) return null;
  }
  for (const [x, y] of [[rect.left + 1, rect.top + 1], [rect.right - 1, rect.top + 1],
    [rect.left + 1, rect.bottom - 1], [rect.right - 1, rect.bottom - 1],
    [rect.left + rect.width / 2, rect.top + rect.height / 2]]) {
    if (!element.contains(document.elementFromPoint(x, y))) return null;
  }
  const x = Math.round(rect.left * devicePixelRatio), y = Math.round(rect.top * devicePixelRatio);
  return { x, y, width: Math.round(rect.right * devicePixelRatio) - x, height: Math.round(rect.bottom * devicePixelRatio) - y };
}
