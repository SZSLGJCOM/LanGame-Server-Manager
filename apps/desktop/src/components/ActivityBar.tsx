import { useEffect, useId, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { isChineseLocale, useI18n } from "../i18n";
import { ActivityNoticeContent } from "./ActivityNotice";
import type { ActivityNoticeStore } from "./activity-notice-store";
import { ShellIcon } from "./ShellIcon";
import { ActivityProgressDetails } from "./ActivityProgress";

export interface ActivityBarEntry {
  id: string;
  content: ReactNode;
}

interface ActivityBarProps {
  store: ActivityNoticeStore;
  entries: ActivityBarEntry[];
  progress?: ReactNode;
}

export function ActivityBar({ store, entries: suppliedEntries, progress }: ActivityBarProps) {
  const { t, locale } = useI18n();
  const copy = isChineseLocale(locale) ? {
    previous: "上一条消息", next: "下一条消息", pause: "暂停轮播", resume: "继续轮播", details: "消息详情"
  } : {
    previous: "Previous message", next: "Next message", pause: "Pause rotation", resume: "Resume rotation", details: "Message details"
  };
  const notices = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
  const dismissLabel = t("servers.mods.closeToast", undefined, "Close");
  const entries: ActivityBarEntry[] = [
    ...notices.map((notice) => ({ id: notice.id, content:
      <ActivityNoticeContent tone={notice.tone} action={notice.action} onDismiss={notice.dismiss}
        dismissLabel={dismissLabel}>{notice.text}</ActivityNoticeContent> })),
    ...suppliedEntries
  ];
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [visible, setVisible] = useState(() => typeof document === "undefined" || !document.hidden);
  const [reducedMotion, setReducedMotion] = useState(() => typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches);
  const container = useRef<HTMLDivElement>(null);
  const expandButton = useRef<HTMLButtonElement>(null);
  const panelClose = useRef<HTMLButtonElement>(null);
  const previousIds = useRef<string[]>([]);
  const panelId = useId();
  const index = Math.max(0, entries.findIndex((entry) => entry.id === selectedId));
  const selected = entries[index];
  const ids = entries.map((entry) => entry.id);
  const signature = JSON.stringify(ids);
  const interacting = hovered || focused || expanded;

  // A retry/dismiss can remove its own focused button without dispatching blur.
  // Keep keyboard users in the bar instead of leaving focus on the document.
  useLayoutEffect(() => {
    if (focused && !container.current?.contains(document.activeElement)) {
      const target = expanded ? panelClose.current : expandButton.current;
      if (target) target.focus();
      else setFocused(false);
    }
  });

  // New operations get a turn; changing progress text within one operation does
  // not restart its animation or steal focus from a recovery action.
  useEffect(() => {
    const added = ids.find((id) => !previousIds.current.includes(id));
    previousIds.current = ids;
    if ((!interacting && added) || !ids.includes(selectedId ?? "")) {
      setSelectedId(added ?? ids[0] ?? null);
    }
  }, [signature, interacting, selectedId]);

  useEffect(() => {
    const onVisibility = () => setVisible(!document.hidden);
    const preference = matchMedia("(prefers-reduced-motion: reduce)");
    const onPreference = () => setReducedMotion(preference.matches);
    document.addEventListener("visibilitychange", onVisibility);
    preference.addEventListener("change", onPreference);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      preference.removeEventListener("change", onPreference);
    };
  }, []);

  useEffect(() => {
    if (entries.length < 2 || paused || interacting || !visible || reducedMotion) return;
    const timer = window.setTimeout(() => setSelectedId(ids[(index + 1) % ids.length]), 8000);
    return () => window.clearTimeout(timer);
  }, [signature, index, paused, interacting, visible, reducedMotion]);

  function closePanel(restoreFocus = true) {
    setExpanded(false);
    if (restoreFocus) expandButton.current?.focus();
    else setFocused(false);
  }

  useEffect(() => {
    if (!expanded) return;
    panelClose.current?.focus();
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !container.current?.contains(event.target)) closePanel(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [expanded]);

  useEffect(() => {
    if (expanded && !entries.length && !progress) closePanel();
  }, [expanded, entries.length, progress]);

  function move(offset: number) {
    setSelectedId(ids[(index + offset + ids.length) % ids.length]);
  }

  const detailsButton = <button type="button" className="shell-activity-expand" ref={expandButton} aria-label={copy.details}
    title={copy.details} aria-expanded={expanded} aria-controls={panelId} onClick={() => setExpanded((value) => !value)}>
    <ShellIcon name="list" />
  </button>;

  return <div className={`shell-activity-content${progress ? " has-progress" : ""}`} ref={container}
    onMouseEnter={() => setHovered(true)} onMouseLeave={() => setHovered(false)}
    onFocusCapture={() => setFocused(true)} onBlurCapture={(event) => {
      if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false);
    }} onKeyDown={(event) => {
      if (event.key === "Escape" && expanded) { event.stopPropagation(); closePanel(); }
    }}>
    {selected ? <div className="shell-activity-message-row">
      <div className="shell-activity-viewport shell-activity-notices" aria-live="off">
        <div className="shell-activity-slide" key={selected.id}>{selected.content}</div>
      </div>
      <div className="shell-activity-controls">
        {entries.length > 1 ? <>
          <button type="button" className="shell-activity-previous" aria-label={copy.previous}
            title={copy.previous} onClick={() => move(-1)}><ShellIcon name="chevron-left" /></button>
          <span className="shell-activity-count" aria-live="off">{index + 1} / {entries.length}</span>
          <button type="button" className="shell-activity-next" aria-label={copy.next}
            title={copy.next} onClick={() => move(1)}><ShellIcon name="chevron-right" /></button>
          {!reducedMotion ? <button type="button" className="shell-activity-pause" aria-label={paused ? copy.resume : copy.pause}
            title={paused ? copy.resume : copy.pause} aria-pressed={paused} onClick={() => setPaused((value) => !value)}>
            <ShellIcon name={paused ? "play" : "pause"} />
          </button> : null}
        </> : null}
        {detailsButton}
      </div>
    </div> : null}
    {progress ? <div className="shell-activity-progress">{progress}</div> : null}
    {!selected && progress ? <div className="shell-activity-controls">{detailsButton}</div> : null}
    {expanded && (entries.length > 0 || progress) ? <section className="shell-activity-panel" id={panelId} role="dialog" aria-label={copy.details}>
      <div className="shell-activity-panel-heading">
        <strong>{copy.details}</strong>
        <button type="button" className="shell-activity-panel-close" ref={panelClose} aria-label={dismissLabel}
          onClick={() => closePanel()}><ShellIcon name="x" /></button>
      </div>
      <div className="shell-activity-panel-list">{entries.map((entry) =>
        <div className="shell-activity-panel-item" key={entry.id}>{entry.content}</div>)}
        {progress ? <div className="shell-activity-panel-item"><ActivityProgressDetails.Provider value={true}>
          {progress}
        </ActivityProgressDetails.Provider></div> : null}
      </div>
    </section> : null}
  </div>;
}
