import { useEffect, useId, useLayoutEffect, useRef, useState, type ButtonHTMLAttributes, type ComponentProps, type ReactNode } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { ConfigurationActionHelp, ConfigurationHelp } from "../settings/ConfigurationFieldHelp";

type IconName = ComponentProps<typeof ShellIcon>["name"];

/** One compact strip per maintenance card: actions and settings chips sit beside the title instead of taking rows. */
export function MaintenanceToolbar({ label, children }: { label: string; children: ReactNode }) {
  return <div className="maintenance-toolbar" role="toolbar" aria-label={label}>{children}</div>;
}

interface ActionProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children" | "disabled"> {
  icon: IconName;
  label: string;
  /** Explanation shown in the shared help bubble. */
  help?: string | null;
  /** When set the action is unavailable and the bubble explains why. */
  disabledReason?: string | null;
  disabled?: boolean;
  iconOnly?: boolean;
  busy?: boolean;
}

/** Small icon (+ label) button; explanations and unavailable reasons use the configuration help bubble. */
export function MaintenanceAction({ icon, label, help, disabledReason, disabled, iconOnly, busy, className, ...rest }: ActionProps) {
  const classes = `${className ?? "ghost-button"} maintenance-action${iconOnly ? " maintenance-action--icon" : ""}`;
  const content = <><ShellIcon name={icon} className="maintenance-action__icon" aria-hidden="true" />{iconOnly ? null : <span>{label}</span>}</>;
  if (disabledReason) {
    return <ConfigurationActionHelp reason={disabledReason}>
      <button type="button" {...rest} className={classes} disabled aria-label={iconOnly ? label : undefined}>{content}</button>
    </ConfigurationActionHelp>;
  }
  return <ConfigurationHelp description={help ?? (iconOnly ? label : null)}>{(bubble) =>
    <button type="button" {...rest} ref={bubble.anchorRef} {...bubble.interactionProps} className={classes}
      disabled={disabled || busy} aria-busy={busy || undefined} aria-label={iconOnly ? label : undefined}
      aria-describedby={bubble.descriptionId}>{content}</button>}</ConfigurationHelp>;
}

/** A titled group inside a maintenance card; its explanation sits in the heading's ⓘ, never as prose. */
export function MaintenanceGroup({ label, help, className, children }: { label: string; help?: string | null; className?: string; children: ReactNode }) {
  return <div className={`maintenance-group${className ? ` ${className}` : ""}`} role="group" aria-label={label}>
    <h4 className="maintenance-group__heading"><MaintenanceHelp help={help}>{label}</MaintenanceHelp></h4>
    {children}
  </div>;
}

/** ⓘ help: the shared configuration bubble on a label (or on the icon alone), for explanations that would otherwise be prose. */
export function MaintenanceHelp({ help, label, className, children }: { help?: string | null; label?: string; className?: string; children?: ReactNode }) {
  if (!help) return children ? <>{children}</> : null;
  return <ConfigurationHelp description={help}>{(bubble) =>
    <span className={`maintenance-help${className ? ` ${className}` : ""}`} ref={bubble.anchorRef} {...bubble.interactionProps} tabIndex={0}
      role={children ? undefined : "img"} aria-label={children ? undefined : label} aria-describedby={bubble.descriptionId}>
      {children}<ShellIcon name="alert-circle" className="maintenance-help-icon" aria-hidden="true" /></span>}</ConfigurationHelp>;
}

interface ChipProps {
  icon: IconName;
  label: string;
  /** Current state at a glance, e.g. "开 · 保留 10". */
  value?: string;
  /** Names the popover's content for styling and tests (e.g. the policy card it replaces). */
  className?: string;
  /** Explanation of the whole popover, shown as ⓘ next to its heading instead of prose inside it. */
  help?: string | null;
  /** Mount the content on first open (for flows that read storage); it then stays mounted. */
  lazy?: boolean;
  /** Pure on/off setting: the chip is the shared settings-toggle-card checkbox and toggles on click; children only carry activity notices. */
  checked?: boolean;
  onCheckedChange?(checked: boolean): void;
  disabled?: boolean;
  busy?: boolean;
  /** The setting does not apply here: the chip stays focusable (aria-disabled) and its bubble explains why; no popover. */
  disabledReason?: string | null;
  children?: ReactNode;
}

const POPOVER_GAP = 6;
const POPOVER_MARGIN = 8;

interface Bounds { left: number; right: number; top: number; bottom: number }

/** The visible area a popover may use: the viewport cut by every clipping (scrolling) ancestor and, horizontally, by
 * the nearest layout container (or the outermost ancestor), so it never spills out of a narrow column. */
function popoverBounds(chip: Element): Bounds {
  const bounds: Bounds = { left: 0, right: document.documentElement.clientWidth, top: 0, bottom: window.innerHeight };
  const clip = (rect: DOMRect, sides: readonly (keyof Bounds)[]) => {
    for (const side of sides) bounds[side] = side === "left" || side === "top" ? Math.max(bounds[side], rect[side]) : Math.min(bounds[side], rect[side]);
  };
  let container: Element | null = null, outermost: Element | null = null;
  for (let parent = chip.parentElement; parent && parent !== document.body; parent = parent.parentElement) {
    const style = getComputedStyle(parent);
    if (style.overflowY !== "visible" || style.overflowX !== "visible") clip(parent.getBoundingClientRect(), ["left", "right", "top", "bottom"]);
    if (!container && style.containerType !== "normal") container = parent;
    outermost = parent;
  }
  const column = container ?? outermost;
  if (column) clip(column.getBoundingClientRect(), ["left", "right"]);
  return bounds;
}

/** Anchors the open panel to the chip: below it when it fits, else flipped above, clamped horizontally to the visible area. */
function placePanel(chip: HTMLElement, panel: HTMLElement) {
  const anchor = chip.getBoundingClientRect();
  if (!anchor.width && !anchor.height) return; // hidden with its section; re-anchored when shown again
  const visible = popoverBounds(chip);
  const left = visible.left + POPOVER_MARGIN, right = visible.right - POPOVER_MARGIN;
  const top = visible.top + POPOVER_MARGIN, bottom = visible.bottom - POPOVER_MARGIN;
  const preferred = parseFloat(getComputedStyle(panel).getPropertyValue("--maintenance-popover-width")) || 420;
  const width = Math.max(160, Math.min(preferred, right - left));
  panel.style.width = `${width}px`;
  // Neither side fits: take the roomier one and let the panel scroll inside it rather than run past the visible area.
  const spaceBelow = bottom - anchor.bottom - POPOVER_GAP, spaceAbove = anchor.top - POPOVER_GAP - top;
  const natural = Math.min(panel.scrollHeight + panel.offsetHeight - panel.clientHeight,
    Math.min(window.innerHeight * 0.7, 520)); // the stylesheet's own cap
  const flip = natural > spaceBelow && (natural <= spaceAbove || spaceAbove > spaceBelow);
  const maxHeight = `${Math.max(120, Math.min(natural, flip ? spaceAbove : spaceBelow))}px`;
  if (panel.style.maxHeight !== maxHeight) panel.style.maxHeight = maxHeight;
  const height = panel.offsetHeight;
  panel.dataset.placement = flip ? "above" : "below";
  panel.style.top = flip ? `${-POPOVER_GAP - height}px` : `${anchor.height + POPOVER_GAP}px`;
  panel.style.left = `${Math.min(Math.max(anchor.left, left), Math.max(left, right - width)) - anchor.left}px`;
}

/** Settings chip. A pure on/off setting is a checkbox chip (the settings-toggle-card dot); anything richer opens its editor in a popover anchored
 * to the chip (out of flow, flipped/clamped to the visible area). The editor stays mounted while closed so its
 * auto-save keeps running and reporting to the activity bar. */
export function MaintenanceChip({ icon, label, value, className, help, lazy, checked, onCheckedChange, disabled, busy, disabledReason, children }: ChipProps) {
  const [open, setOpen] = useState(false);
  const [visited, setVisited] = useState(!lazy);
  if (open && !visited) setVisited(true);
  const root = useRef<HTMLSpanElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const panelId = useId();
  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      const target = event.target as Element | null;
      // Help bubbles are portalled to the body; reading one must not close its popover.
      if (target && !root.current?.contains(target) && !target.closest?.("[data-configuration-help-anchor]")) setOpen(false);
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [open]);
  // Re-anchor on every render while open (content and column width change with it), and on scroll/resize.
  useLayoutEffect(() => { if (open && root.current && panel.current) placePanel(root.current, panel.current); });
  useEffect(() => {
    if (!open || !root.current || !panel.current) return;
    const chip = root.current, element = panel.current;
    let frame = 0;
    const update = () => { cancelAnimationFrame(frame); frame = requestAnimationFrame(() => placePanel(chip, element)); };
    // The chip is observed too: a preserved section shown again (0 → size) re-anchors; placing in a frame keeps the
    // observer from re-entering on the panel size it just set.
    const observer = new ResizeObserver(update);
    for (const target of [element, chip, chip.parentElement]) if (target) observer.observe(target);
    // Section and sidebar transitions move the chip without resizing anything.
    const moves = ["resize", "scroll", "transitionend", "animationend"] as const;
    for (const type of moves) window.addEventListener(type, update, true);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      for (const type of moves) window.removeEventListener(type, update, true);
    };
  }, [open]);
  const classes = `maintenance-chip${checked === undefined ? "" : " maintenance-chip--toggle"}${className ? ` ${className}` : ""}`;
  if (disabledReason) {
    return <span className={`${classes} is-unavailable`}>
      <ConfigurationHelp description={disabledReason}>{(bubble) =>
        <button type="button" ref={bubble.anchorRef} {...bubble.interactionProps} className="maintenance-chip__summary"
          aria-disabled="true" aria-describedby={bubble.descriptionId}>
          <ShellIcon name={icon} className="maintenance-action__icon" aria-hidden="true" />
          <span>{label}</span>
        </button>}</ConfigurationHelp>
    </span>;
  }
  if (checked !== undefined) {
    return <span className={classes}>
      {/* Same selection markup as the daily-maintenance toggle: a settings-toggle-card with its checkbox dot. */}
      <label className="maintenance-chip__summary settings-toggle-card" aria-busy={busy || undefined}>
        <ShellIcon name={icon} className="maintenance-action__icon" aria-hidden="true" />
        <span>{label}</span>
        {value ? <span className="maintenance-chip__value">{value}</span> : null}
        <input type="checkbox" checked={checked} disabled={disabled || busy}
          onChange={(event) => onCheckedChange?.(event.target.checked)} />
      </label>
      {children}
    </span>;
  }
  return <span ref={root} className={classes} onKeyDown={(event) => {
    if (event.key !== "Escape" || !open) return;
    event.stopPropagation();
    setOpen(false);
    button.current?.focus();
  }}>
    <button ref={button} type="button" className="maintenance-chip__summary" aria-expanded={open} aria-controls={panelId}
      onClick={() => setOpen((value) => !value)}>
      <ShellIcon name={icon} className="maintenance-action__icon" aria-hidden="true" />
      <span>{label}</span>
      {value ? <span className="maintenance-chip__value">{value}</span> : null}
    </button>
    <div ref={panel} id={panelId} className="maintenance-chip__panel" role="group" aria-label={label} hidden={!open}>
      {help ? <div className="maintenance-chip__heading"><MaintenanceHelp help={help}>{label}</MaintenanceHelp></div> : null}
      {visited ? children : null}
    </div>
  </span>;
}
