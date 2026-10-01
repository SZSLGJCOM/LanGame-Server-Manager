import type { CSSProperties } from "react";

type AssistantPanelStyle = CSSProperties & {
  "--assistant-panel-left": string;
  "--assistant-panel-top": string;
  "--assistant-panel-width": string;
};

// The expanded island keeps the capsule's top edge and horizontal center.
// Loading, failure and chat share this geometry; the page header is not its anchor.
export function assistantPanelStyle(
  anchorRect: DOMRect | null | undefined,
  viewportWidth = typeof window === "undefined" ? 0 : window.innerWidth
): AssistantPanelStyle | undefined {
  if (!anchorRect || !viewportWidth) return undefined;

  const margin = 20;
  const width = Math.min(480, Math.max(280, viewportWidth - margin * 2));
  const idealLeft = anchorRect.left + anchorRect.width / 2 - width / 2;
  const maxLeft = Math.max(margin, viewportWidth - width - margin);
  const left = Math.min(Math.max(idealLeft, margin), maxLeft);

  return {
    "--assistant-panel-left": `${Math.round(left)}px`,
    "--assistant-panel-top": `${Math.round(Math.max(0, anchorRect.top))}px`,
    "--assistant-panel-width": `${Math.round(width)}px`
  };
}
