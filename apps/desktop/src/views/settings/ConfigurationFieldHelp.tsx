import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type HTMLAttributes,
  type ReactNode
} from "react";
import { createPortal } from "react-dom";
import type { TranslateFn } from "../../i18n";
import { useHelpTooltipInteraction } from "./useHelpTooltipInteraction";

const TOOLTIP_GAP = 8;
const TOOLTIP_MAX_WIDTH = 320;
const SOURCE_ONLY_HELP_PATTERN = /^(?!.*[;；])(?:Native\b.*\b(?:setting|option|parameter|field)(?:\s+[A-Za-z0-9_.-]+|\s+from\s+[A-Za-z0-9 _.-]+)?\.?$|.*\bbuild\s+\d+\s+native\b|(?:Written|Stored|Serialized|Rendered|Mapped|Emitted|Read by|Consumed by|Passed as)\b|This (?:value|setting|option) is (?:written|stored|serialized|mapped)\b|(?:此项|该值|此值)(?:会)?(?:写入|保存到|映射到)|从.+派生该值[，,].+读取|作为\s*-\S+\s*参数传入)/i;

export type ConfigurationFieldHelpMode = "summary" | "instructions";

interface TooltipPosition {
  left: number;
  maxWidth: number;
  placement: "above" | "below";
  top: number;
}

interface TooltipAnchorRect {
  bottom: number;
  left: number;
  right: number;
  top: number;
}

interface TooltipSize {
  height: number;
  width: number;
}

interface TooltipViewport {
  height: number;
  width: number;
}

export interface ConfigurationFieldHelpBinding {
  anchorRef: (node: HTMLElement | null) => void;
  descriptionId?: string;
  helpNode: ReactNode;
  interactionProps: Pick<
    HTMLAttributes<HTMLElement>,
    "onBlurCapture" | "onFocusCapture" | "onKeyDownCapture" | "onPointerEnter" | "onPointerOver" | "onPointerLeave" | "onPointerDownCapture"
  >;
}

function playerFacingFallback(t: TranslateFn | undefined, title?: string): string | null {
  const normalized = title?.replace(/\s+/g, " ").trim();
  if (!normalized) return null;
  return t?.(
    "settings.configuration.fieldHelp.fallback",
    { title: normalized },
    "Controls {title} for this server."
  ) ?? `Controls ${normalized} for this server.`;
}

function repeatsFieldLabel(text: string | null, title: string | undefined, t?: TranslateFn): boolean {
  if (!text || !title?.trim()) return false;
  const normalize = (value: string) => value.replace(/\s+/g, " ").trim()
    .replace(/[A-Z]/g, (letter) => letter.toLowerCase());
  const candidate = normalize(text);
  const fallback = playerFacingFallback(t, title);
  return candidate === `控制 ${normalize(title)}。` ||
    (fallback !== null && candidate === normalize(fallback));
}

export function summarizeConfigurationFieldHelp(
  description?: string | null,
  title?: string,
  t: TranslateFn = (_key, params, fallback) => String(fallback ?? "").replace(
    /\{\s*title\s*\}/g,
    String(params?.title ?? "")
  ),
  mode: ConfigurationFieldHelpMode = "summary"
): string | null {
  const normalized = description?.trim();
  if (!normalized) return null;
  if (mode === "instructions") return repeatsFieldLabel(normalized, title, t) ? null : normalized;
  // File provenance is available separately; units, conditions and native syntax remain useful help.
  const sentences = normalized.replace(/\s+/g, " ").match(/.+?(?:[。！？!?]|\.(?=\s|$)|$)/gu) ?? [];
  const help = sentences.filter((sentence) => sentence.trim()
    && !repeatsFieldLabel(sentence, title, t) && !SOURCE_ONLY_HELP_PATTERN.test(sentence.trim()))
    .join("").trim();
  return help || null;
}

export function resolveTooltipPosition(
  anchor: TooltipAnchorRect,
  tooltip: TooltipSize,
  viewport: TooltipViewport
): TooltipPosition {
  const maxWidth = Math.min(
    TOOLTIP_MAX_WIDTH,
    Math.max(1, viewport.width - TOOLTIP_GAP * 2)
  );
  const measuredWidth = tooltip.width > 0 ? Math.min(tooltip.width, maxWidth) : maxWidth;
  const measuredHeight = Math.max(0, tooltip.height);
  const left = Math.min(
    Math.max(anchor.left, TOOLTIP_GAP),
    Math.max(TOOLTIP_GAP, viewport.width - measuredWidth - TOOLTIP_GAP)
  );
  const belowTop = anchor.bottom + TOOLTIP_GAP;
  const aboveTop = anchor.top - TOOLTIP_GAP - measuredHeight;
  const belowFits = belowTop + measuredHeight <= viewport.height - TOOLTIP_GAP;
  const aboveFits = aboveTop >= TOOLTIP_GAP;
  const placement = !belowFits && aboveFits ? "above" : "below";
  const preferredTop = placement === "above" ? aboveTop : belowTop;
  const top = Math.min(
    Math.max(preferredTop, TOOLTIP_GAP),
    Math.max(TOOLTIP_GAP, viewport.height - measuredHeight - TOOLTIP_GAP)
  );
  return {
    left,
    maxWidth,
    placement,
    top
  };
}

export function useConfigurationFieldHelp(
  id: string,
  description?: string | null,
  title?: string,
  t?: TranslateFn,
  mode: ConfigurationFieldHelpMode = "summary",
  shouldShow?: () => boolean
): ConfigurationFieldHelpBinding {
  const summary = summarizeConfigurationFieldHelp(description, title, t, mode);
  const text = repeatsFieldLabel(summary, title, t) ? null : summary;
  const anchorElement = useRef<HTMLElement | null>(null);
  const tooltipElement = useRef<HTMLSpanElement | null>(null);
  const entranceFrame = useRef<number | null>(null);
  const layoutFrame = useRef<number | null>(null);
  const [position, setPosition] = useState<TooltipPosition>({
    left: 0, top: 0, maxWidth: TOOLTIP_MAX_WIDTH, placement: "below"
  });
  const descriptionId = text ? id : undefined;
  const readPosition = useCallback((measureTooltip: boolean): TooltipPosition | null => {
    if (!anchorElement.current || typeof document === "undefined" || typeof window === "undefined") {
      return null;
    }
    const positioningAnchor = anchorElement.current.querySelector<HTMLElement>(
      ".configuration-field-control"
    ) ?? anchorElement.current;
    if (!positioningAnchor.isConnected || positioningAnchor.closest("[hidden], [inert]")
      || !positioningAnchor.getClientRects().length || getComputedStyle(positioningAnchor).visibility === "hidden") {
      return null;
    }
    const anchorRect = positioningAnchor.getBoundingClientRect();
    const viewport = {
      height: Math.max(document.documentElement.clientHeight, window.innerHeight || 0),
      width: Math.max(document.documentElement.clientWidth, window.innerWidth || 0)
    };
    if (
      anchorRect.bottom <= 0 || anchorRect.right <= 0 ||
      anchorRect.top >= viewport.height || anchorRect.left >= viewport.width
    ) {
      return null;
    }
    const tooltipRect = measureTooltip ? tooltipElement.current?.getBoundingClientRect() : null;
    return resolveTooltipPosition(
      anchorRect,
      { height: tooltipRect?.height ?? 0, width: tooltipRect?.width ?? 0 },
      viewport
    );
  }, []);

  const { phase, interactionProps, bubbleProps, dismiss, markVisible } = useHelpTooltipInteraction(
    Boolean(text), anchorElement, tooltipElement,
    () => shouldShow?.() !== false && readPosition(false) !== null
  );
  const hasTooltip = phase === "measuring" || phase === "visible" || phase === "leaving";

  useLayoutEffect(() => {
    if (phase !== "measuring") return;
    const measuredPosition = readPosition(true);
    if (!measuredPosition) {
      dismiss();
      return;
    }
    setPosition(measuredPosition);
    entranceFrame.current = window.requestAnimationFrame(markVisible);
    return () => {
      if (entranceFrame.current !== null) window.cancelAnimationFrame(entranceFrame.current);
      entranceFrame.current = null;
    };
  }, [dismiss, markVisible, phase, readPosition]);

  useEffect(() => {
    if (phase === null || phase === "leaving") return;
    const schedulePositionUpdate = () => {
      if (layoutFrame.current !== null) return;
      layoutFrame.current = window.requestAnimationFrame(() => {
        layoutFrame.current = null;
        const nextPosition = readPosition(true);
        if (!nextPosition || shouldShow?.() === false) {
          dismiss();
          return;
        }
        setPosition((current) => current.left === nextPosition.left && current.top === nextPosition.top
          && current.maxWidth === nextPosition.maxWidth && current.placement === nextPosition.placement
          ? current : nextPosition);
      });
    };
    window.addEventListener("resize", schedulePositionUpdate);
    window.addEventListener("scroll", schedulePositionUpdate, true);
    // Mounted categories may become hidden without unmounting their help portal.
    const observer = new MutationObserver(schedulePositionUpdate);
    for (let node = anchorElement.current; node; node = node.parentElement) {
      observer.observe(node, { attributes: true, attributeFilter: ["hidden", "inert", "class", "style"] });
    }
    const resizeObserver = new ResizeObserver(schedulePositionUpdate);
    if (anchorElement.current) resizeObserver.observe(anchorElement.current);
    if (tooltipElement.current) resizeObserver.observe(tooltipElement.current);
    schedulePositionUpdate();
    return () => {
      window.removeEventListener("resize", schedulePositionUpdate);
      window.removeEventListener("scroll", schedulePositionUpdate, true);
      observer.disconnect();
      resizeObserver.disconnect();
      if (layoutFrame.current !== null) window.cancelAnimationFrame(layoutFrame.current);
      layoutFrame.current = null;
    };
  }, [dismiss, phase, readPosition, shouldShow, text]);

  const anchorRef = useCallback((node: HTMLElement | null) => {
    anchorElement.current?.removeAttribute("data-configuration-help-anchor");
    anchorElement.current = node;
    if (text) node?.setAttribute("data-configuration-help-anchor", "");
  }, [text]);
  const helpNode = text ? (
    <>
      <span id={id} className="configuration-field-help-a11y" role="tooltip">{text}</span>
      {hasTooltip && typeof document !== "undefined" ? createPortal(
        <span
          aria-hidden="true"
          data-configuration-help-anchor=""
          ref={tooltipElement}
          className={`configuration-field-help-tooltip is-${position.placement} is-${phase}`}
          style={{ left: position.left, maxWidth: position.maxWidth, top: position.top }}
          {...bubbleProps}
        >
          {text}
        </span>,
        document.body
      ) : null}
    </>
  ) : null;

  return {
    anchorRef,
    descriptionId,
    helpNode,
    interactionProps
  };
}

/** Apply help to the existing element so grid/flex ownership stays unchanged. */
export function ConfigurationHelp(props: {
  description?: string | null;
  children: (help: ConfigurationFieldHelpBinding) => ReactNode;
}) {
  const help = useConfigurationFieldHelp(useId(), props.description, undefined, undefined, "instructions");
  return <>{props.children(help)}{help.helpNode}</>;
}
