export function checkStoragePathLayout(
  root: ParentNode,
  check: (condition: unknown, message: string) => void,
) {
  const entries = [...root.querySelectorAll<HTMLElement>(".system-path-row")];
  check(entries.length === 4, "Storage settings must show all four path roles");
  const bounds = entries.map(entry => entry.getBoundingClientRect());
  check(bounds.every((rect, index) => rect.width > 0 && Math.abs(rect.top - bounds[0].top) <= 1
    && (index === 0 || rect.left >= bounds[index - 1].right - 1)),
  `Storage paths must remain side by side at ${innerWidth}px: ${JSON.stringify(bounds.map(rect => rect.toJSON()))}`);

  for (const [index, entry] of entries.entries()) {
    const rect = bounds[index];
    const label = entry.querySelector<HTMLElement>(".system-runtime-label");
    const path = entry.querySelector<HTMLElement>(".system-runtime-path");
    check(label && label.scrollWidth <= label.clientWidth + 1,
      "Every path label must fit its column without clipping");
    check(path && path.clientWidth > 0 && path.title === path.textContent,
      "Every path must retain visible space and its full-path tooltip");
    for (const button of entry.querySelectorAll<HTMLButtonElement>("button")) {
      const control = button.getBoundingClientRect();
      check(control.width >= 28 && control.height >= 28
        && control.left >= rect.left - 1 && control.right <= rect.right + 1
        && control.top >= rect.top - 1 && control.bottom <= rect.bottom + 1
        && button.scrollWidth <= button.clientWidth + 1,
      `Path actions must fit their own column: ${button.getAttribute("aria-label")}`);
    }
  }
}
