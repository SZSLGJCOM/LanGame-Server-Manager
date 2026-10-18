import { createContext, useContext, useId, useLayoutEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { ShellIcon } from "./ShellIcon";
import type { ActivityNoticeStore } from "./activity-notice-store";

// The shell owns the target; notices disappear with their originating view.
// Undefined permits isolated editors to render accessible feedback without a shell.
export const ActivityNoticeTarget = createContext<{
  element: HTMLElement | null;
  dismissLabel: string;
  store?: ActivityNoticeStore;
} | undefined>(undefined);

export const ActivityNoticeDetails = createContext(false);

interface ActivityNoticeProps {
  children: string;
  details?: string;
  tone?: "error" | "warning" | "success" | "info";
  action?: ReactNode;
  onDismiss?: () => void;
}

export function ActivityNotice({ children, details, tone = "info", action, onDismiss }: ActivityNoticeProps) {
  const host = useContext(ActivityNoticeTarget);
  const id = useId();
  const target = host?.element;
  const store = host?.store;
  const [dismissal, setDismissal] = useState({ message: children, hidden: false });
  if (dismissal.message !== children) setDismissal({ message: children, hidden: false });
  const hidden = !children || (dismissal.message === children && dismissal.hidden);
  const dismiss = () => { setDismissal({ message: children, hidden: true }); onDismiss?.(); };
  useLayoutEffect(() => {
    if (!store) return;
    if (hidden) store.remove(id);
    else store.publish({ id, text: children, details, tone, action, dismiss });
  }, [store, id, children, details, tone, action, onDismiss, hidden]);
  useLayoutEffect(() => () => store?.remove(id), [store, id]);
  if (hidden || store || target === null) return null;
  const notice = <ActivityNoticeContent tone={tone} details={details} action={action} onDismiss={dismiss}
    dismissLabel={host?.dismissLabel ?? "Close"}>{children}</ActivityNoticeContent>;
  return target === undefined ? notice : createPortal(notice, target);
}

export function ActivityNoticeContent({ children, details, tone = "info", action, onDismiss, dismissLabel }: ActivityNoticeProps & { dismissLabel: string }) {
  const showDetails = useContext(ActivityNoticeDetails);
  return <div className={`shell-activity-notice is-${tone}`} role={tone === "error" ? "alert" : "status"}>
    <span className="shell-activity-notice-copy">
      <span className="shell-activity-notice-text" title={children}>{children}</span>
      {showDetails && details ? <span className="shell-activity-notice-diagnostics">{details}</span> : null}
    </span>
    {action ? <span className="shell-activity-notice-actions">{action}</span> : null}
    <button type="button" className="shell-activity-notice-close" aria-label={dismissLabel} onClick={onDismiss}>
      <ShellIcon name="x" />
    </button>
  </div>;
}
