import { createContext, useContext } from "react";
import { ShellIcon } from "./ShellIcon";

export const ActivityProgressDetails = createContext(false);

interface ActivityProgressProps {
  label: string;
  detail?: string;
  title?: string;
  active: boolean;
  error?: boolean;
  percent: number | null;
  transfer?: string;
  speed?: string;
  speedLabel?: string;
  speedTitle?: string;
  elapsedLabel?: string;
  onStop?: () => void;
  stopRequested?: boolean;
  stopLabel?: string;
}

export function ActivityProgress(props: ActivityProgressProps) {
  const showDetails = useContext(ActivityProgressDetails);
  const percent = props.percent !== null && Number.isFinite(props.percent)
    ? Math.max(0, Math.min(100, props.percent)) : null;
  return (
    <div className={`shell-task-activity${props.active ? " is-active" : ""}${props.error ? " is-error" : ""}`}>
      <div className="shell-task-summary">
        <span className="shell-activity-text" title={props.title} role={props.error ? "alert" : undefined}>
          <span className="shell-task-phase">{props.label}</span>
          {props.detail ? <> <span className="shell-task-detail">{props.detail}</span></> : null}
        </span>
      </div>
      {showDetails && props.title ? <span className="shell-task-diagnostics">{props.title}</span> : null}
      {props.active ? <>
        <div className={`shell-task-meter${percent === null ? " is-indeterminate" : ""}`}
          role="progressbar" aria-label={props.label} aria-valuemin={0} aria-valuemax={100}
          aria-valuenow={percent ?? undefined} aria-live="off">
          <span className="shell-task-meter-fill" style={percent === null ? undefined : { width: `${percent}%` }} />
        </div>
        <span className="shell-task-percent" aria-hidden="true">{percent === null ? "—" : `${Math.floor(percent)}%`}</span>
        <span className="shell-task-transfer" title={props.transfer || undefined} aria-live="off">{props.transfer}</span>
        <span className="shell-task-speed" title={props.speedTitle} aria-label={props.speed ? `${props.speedLabel}: ${props.speed}` : undefined}
          aria-live="off">{props.speed}</span>
      </> : null}
      <span className="shell-task-elapsed" role="timer" title={props.elapsedLabel} aria-label={props.elapsedLabel} aria-live="off">
        {props.elapsedLabel}
      </span>
      {props.active && (props.onStop || props.stopRequested) ? (
        <button className="shell-task-stop" type="button" onClick={props.onStop} disabled={props.stopRequested}
          title={props.stopLabel} aria-label={props.stopLabel} aria-busy={props.stopRequested || undefined}>
          <ShellIcon name="square" />
        </button>
      ) : null}
    </div>
  );
}
