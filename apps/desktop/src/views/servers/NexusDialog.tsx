import { useEffect, useId, useRef, type ReactNode } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import "./nexus-mods.css";

export function NexusDialog(props: { title: string; closeLabel: string; busy: boolean; onClose(): void; children: ReactNode; className?: string }) {
  const node = useRef<HTMLDialogElement>(null), title = useId();
  useEffect(() => {
    const dialog = node.current;
    if (!dialog) return;
    const previous = document.activeElement;
    dialog.showModal();
    return () => { dialog.close(); if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); };
  }, []);
  return <dialog ref={node} className={`nexus-dialog ${props.className ?? ""}`} aria-labelledby={title} aria-busy={props.busy} data-no-window-drag="true"
    onCancel={(event) => { event.preventDefault(); if (!props.busy) props.onClose(); }}>
    <header className="nexus-dialog__header"><h2 id={title}>{props.title}</h2>
      <button type="button" className="mw-ghost-btn" disabled={props.busy} onClick={props.onClose} aria-label={props.closeLabel}><ShellIcon name="x" /></button>
    </header>{props.children}
  </dialog>;
}
