import { useEffect, useLayoutEffect, useRef } from "react";

function revealSelectedTab(element: HTMLElement) {
  const selected = element.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
  if (!selected) return;
  const fade = 28;
  // Position inside the scrolled content, independent of the tab's offsetParent.
  const left = selected.getBoundingClientRect().left - element.getBoundingClientRect().left - element.clientLeft + element.scrollLeft;
  const width = selected.getBoundingClientRect().width;
  const right = left + width;
  const view = element.clientWidth;
  // A wide tab keeps its start and focus ring visible across repeated resize notifications.
  if (width + 2 * fade > view) {
    element.scrollLeft = Math.max(0, left - Math.max(3, (view - width) / 2));
    return;
  }
  if (left - fade < element.scrollLeft) element.scrollLeft = Math.max(0, left - fade);
  else if (right + fade > element.scrollLeft + view) element.scrollLeft = right + fade - view;
}

/**
 * Keeps a single-row tab strip usable when its tabs are wider than the space beside the header actions:
 * the strip scrolls horizontally (no visible scrollbar), marks which edges hide tabs so CSS can fade them,
 * maps a vertical mouse wheel to horizontal scrolling, and keeps the selected tab in view.
 */
export function useTabStripOverflow<T extends HTMLElement>(selectedKey: string, itemsKey: string) {
  const strip = useRef<T | null>(null);

  useEffect(() => {
    const element = strip.current;
    if (!element) return;
    const update = () => {
      const max = element.scrollWidth - element.clientWidth;
      const start = max > 1 && element.scrollLeft > 1;
      const end = max > 1 && element.scrollLeft < max - 1;
      if (element.dataset.overflowStart !== String(start)) element.dataset.overflowStart = String(start);
      if (element.dataset.overflowEnd !== String(end)) element.dataset.overflowEnd = String(end);
    };
    const wheel = (event: WheelEvent) => {
      if (event.ctrlKey || Math.abs(event.deltaX) >= Math.abs(event.deltaY)) return;
      const max = element.scrollWidth - element.clientWidth;
      if (max <= 1) return;
      const next = Math.min(max, Math.max(0, element.scrollLeft + event.deltaY));
      if (next === element.scrollLeft) return;
      event.preventDefault();
      element.scrollLeft = next;
    };
    update();
    element.addEventListener("scroll", update, { passive: true });
    element.addEventListener("wheel", wheel, { passive: false });
    // Fonts, translations and header actions can resize the strip without changing selection.
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => {
      revealSelectedTab(element);
      update();
    });
    observer?.observe(element);
    for (const child of element.children) observer?.observe(child);
    return () => {
      element.removeEventListener("scroll", update);
      element.removeEventListener("wheel", wheel);
      observer?.disconnect();
    };
  }, [itemsKey]);

  // The selected tab is scrolled fully into the strip only (never the page), including after keyboard moves.
  useLayoutEffect(() => {
    const element = strip.current;
    if (!element) return;
    revealSelectedTab(element);
    element.dispatchEvent(new Event("scroll"));
  }, [selectedKey, itemsKey]);

  return strip;
}
