const FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "a[href]",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "summary",
  '[tabindex]:not([tabindex="-1"])'
].join(",");

export function listAssistantFocusableElements(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter((element) => {
    if (element.tabIndex < 0 || element.closest('[hidden], [inert], [aria-hidden="true"]')) return false;
    // Only a closed disclosure's own summary can receive focus. Its links and
    // nested disclosures stay out of the dialog's keyboard loop until opened.
    let closedDetails = element.closest<HTMLDetailsElement>("details:not([open])");
    while (closedDetails) {
      if (!closedDetails.querySelector(":scope > summary")?.contains(element)) return false;
      closedDetails = closedDetails.parentElement?.closest<HTMLDetailsElement>("details:not([open])") ?? null;
    }
    return true;
  });
}

export function focusAssistantPanel(panel: HTMLElement) {
  const input = panel.querySelector<HTMLTextAreaElement>(".assistant-chat-input");
  (input ?? listAssistantFocusableElements(panel)[0] ?? panel).focus({ preventScroll: true });
}

export function containAssistantFocus(
  event: Pick<KeyboardEvent, "key" | "shiftKey" | "preventDefault">,
  container: HTMLElement,
  activeElement: Element | null
) {
  if (event.key !== "Tab") {
    return;
  }

  const elements = listAssistantFocusableElements(container);
  if (elements.length === 0) {
    event.preventDefault();
    container.focus({ preventScroll: true });
    return;
  }

  const first = elements[0];
  const last = elements[elements.length - 1];
  const focusIsOutside = !activeElement || activeElement === container || !container.contains(activeElement);
  if (event.shiftKey && (focusIsOutside || activeElement === first)) {
    event.preventDefault();
    last.focus({ preventScroll: true });
  } else if (!event.shiftKey && (focusIsOutside || activeElement === last)) {
    event.preventDefault();
    first.focus({ preventScroll: true });
  }
}
