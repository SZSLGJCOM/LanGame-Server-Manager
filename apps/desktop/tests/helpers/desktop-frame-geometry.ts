function element(root: HTMLElement, selector: string): HTMLElement {
  const target = root.querySelector<HTMLElement>(selector);
  if (!target) throw new Error(`Missing frame element: ${selector}`);
  return target;
}

function aligned(actual: number, expected: number, description: string) {
  if (Math.abs(actual - expected) > 1) {
    throw new Error(`${description}: ${actual} instead of ${expected}`);
  }
}

export function assertShellFrameSpacing(root: HTMLElement) {
  const capsule = element(root, ".assistant-capsule").getBoundingClientRect();
  const navigation = element(root, ".shell-topnav").getBoundingClientRect();
  const scroll = element(root, ".shell-content-scroll");
  const body = element(root, ".shell-content-body");
  const activity = element(root, ".shell-activity-bar").getBoundingClientRect();
  const bounds = body.getBoundingClientRect();
  const style = getComputedStyle(body);
  aligned(navigation.top - capsule.bottom, 8, "LAN and navigation must have an 8px gap");
  aligned(scroll.getBoundingClientRect().bottom, activity.top, "The activity bar must follow the content viewport");
  // Loading and empty states deliberately center themselves across the whole viewport.
  if (parseFloat(style.paddingTop) === 0) return;
  aligned(bounds.top + scroll.scrollTop + parseFloat(style.paddingTop) - navigation.bottom, 8,
    "Navigation and content must use the same 8px gap as LAN and navigation");
  aligned(parseFloat(style.paddingBottom), 8, "Content must retain an 8px bottom inset when scrolling");
  if (Math.abs(bounds.height - scroll.clientHeight) <= 1) {
    aligned(activity.top - (bounds.bottom + scroll.scrollTop - parseFloat(style.paddingBottom)), 8,
      "The content-to-activity gap must match the navigation-to-content gap");
  }
}

export function assertServerDetailFrame(root: HTMLElement) {
  const panel = root.querySelector<HTMLElement>(".server-detail-scroll");
  if (!panel) return;
  const list = element(root, ".server-list-panel");
  const tabs = element(root, ".server-detail-tabs").getBoundingClientRect();
  const bounds = panel.getBoundingClientRect();
  aligned(bounds.left, tabs.left, "The selected workspace must start at its tab strip's left edge");
  aligned(bounds.right, tabs.right, "The selected workspace must end at its tab strip's right edge");
  aligned(bounds.bottom, list.getBoundingClientRect().bottom, "Every detail tab must fill the same height as the instance list");
  const metrics = root.querySelector<HTMLElement>(".workspace-metrics");
  if (metrics) aligned(bounds.right, metrics.getBoundingClientRect().right,
    "Every detail tab must align with the registered-ports card's outer right edge");
  const style = getComputedStyle(panel);
  const listStyle = getComputedStyle(list);
  for (const corner of ["borderTopLeftRadius", "borderTopRightRadius", "borderBottomLeftRadius", "borderBottomRightRadius"] as const) {
    aligned(parseFloat(style[corner]), 12, "All detail frame corners must use the shared 12px radius");
    aligned(parseFloat(style[corner]), parseFloat(listStyle[corner]), "Detail and list corners must match");
  }
  for (const side of ["borderTopWidth", "borderRightWidth", "borderBottomWidth", "borderLeftWidth"] as const) {
    if (style[side] !== "1px" || style[side] !== listStyle[side]) {
      throw new Error(`Detail and list borders must share a 1px stroke: ${side} = ${style[side]}`);
    }
  }
  if (getComputedStyle(panel).overflowY === "hidden") {
    aligned(panel.clientWidth, panel.offsetWidth - 2, "Internally scrolled tabs must not reserve another outer scrollbar gutter");
  }
}
