import { useEffect, useRef, type ReactNode } from "react";
import { ModuleCover } from "../../components/ModuleCover";

interface Props {
  id: string;
  name: string;
  moduleId: string;
  active: boolean;
  status: string;
  statusLabel: string;
  kind?: "instance" | "archive" | "deletion-error";
  onSelect: () => void;
  actions?: ReactNode;
  metadata?: ReactNode;
  description?: string;
  primary: ReactNode;
}

export function ServerCardFrame(props: Props) {
  const card = useRef<HTMLElement>(null);
  useEffect(() => {
    if (props.active) card.current?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [props.active]);
  return <article ref={card} className={`server-list-card ${props.active ? "is-active" : ""}`}
    data-card-kind={props.kind ?? "instance"} data-card-id={props.id}
    data-archive-id={props.kind === "archive" ? props.id : undefined}
    data-deletion-id={props.kind === "deletion-error" ? props.id : undefined}>
    <div className="server-list-card-shell">
      <div className="server-list-card-media">
        <ModuleCover moduleId={props.moduleId} moduleName={props.moduleId} subtitle={props.id}
          showOverlay={false} variant="server-list" />
      </div>
      <button type="button" className="server-list-card-hitarea" onClick={props.onSelect}
        aria-pressed={props.active} aria-label={`${props.name} ${props.statusLabel}`}
        aria-description={props.description} title={props.description} />
      <div className="server-list-card-overlay">
        <div className="server-list-card-toolbar">
          <span className="server-list-card-status" data-status={props.status.toLowerCase()}>
            <span className="server-list-card-status-dot" />{props.statusLabel}
          </span>
          {props.actions}
        </div>
        <div className="server-list-card-footer">
          <div className="server-list-card-copy"><div className="row-title" title={props.name}>{props.name}</div></div>
          {props.metadata}
          {props.primary}
        </div>
      </div>
    </div>
  </article>;
}
