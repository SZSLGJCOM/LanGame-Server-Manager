import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import type { AppUpdateState } from "../types";
import { appUpdateCopy } from "./app-update-copy";
import { containAssistantFocus } from "./assistant-focus";
import { ShellIcon } from "./ShellIcon";
import "./app-update.css";

interface AppUpdatePromptProps {
  enabled: boolean;
  state: AppUpdateState;
  retryRequest: number;
  onCheck: () => void;
  onInstall: () => void;
  onDownloadInstaller: () => Promise<void>;
}

export function AppUpdatePrompt({ enabled, state, retryRequest, onCheck, onInstall, onDownloadInstaller }: AppUpdatePromptProps) {
  const { locale } = useI18n();
  const copy = appUpdateCopy(locale);
  const [openVersion, setOpenVersion] = useState<string | null>(null);
  const [downloadError, setDownloadError] = useState<string | null>(null);
  const [openingDownload, setOpeningDownload] = useState(false);
  const seenVersions = useRef<string[]>([]);
  const lastRetryRequest = useRef(retryRequest);
  const submitted = useRef(false);
  const downloadRequest = useRef(0);
  const downloadPending = useRef(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const title = useRef<HTMLHeadingElement>(null);
  const titleId = useId();
  const detailId = useId();
  const version = state.availableVersion?.trim() || null;
  const actionable = enabled && Boolean(version) && ["available", "checking", "downloading", "installing", "failed"].includes(state.status);
  const open = actionable && openVersion === version;
  const busy = state.status === "checking" || state.status === "downloading" || state.status === "installing";
  const percent = state.contentLength || state.downloadPercent === 100 ? state.downloadPercent : undefined;

  useEffect(() => {
    if (!actionable) { setOpenVersion(null); return; }
    if (state.status === "available" && version && !seenVersions.current.includes(version)) {
      // Session-only, bounded memory: repeated background checks do not reopen a dismissed release.
      seenVersions.current = [...seenVersions.current.slice(-31), version];
      setOpenVersion(version);
    }
  }, [actionable, state.status, version]);

  useEffect(() => {
    if (retryRequest === lastRetryRequest.current) return;
    lastRetryRequest.current = retryRequest;
    if (actionable) setOpenVersion(version);
  }, [retryRequest, actionable, version]);

  useLayoutEffect(() => {
    downloadRequest.current++;
    downloadPending.current = false;
    submitted.current = false;
    setOpeningDownload(false);
    setDownloadError(null);
    return () => { downloadRequest.current++; downloadPending.current = false; };
  }, [version, open]);

  useEffect(() => {
    if (state.status !== "available") submitted.current = false;
  }, [state.status]);

  useLayoutEffect(() => {
    if (!open || !dialog.current) return;
    const node = dialog.current;
    const previous = document.activeElement;
    node.showModal();
    title.current?.focus();
    return () => {
      node.close();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus({ preventScroll: true });
    };
  }, [open]);

  useLayoutEffect(() => {
    if (open && dialog.current && !dialog.current.contains(document.activeElement)) title.current?.focus();
  }, [open, state.status]);

  function close() { setOpenVersion(null); }

  function install() {
    if (!enabled || state.status !== "available" || !version || submitted.current || downloadPending.current) return;
    submitted.current = true;
    onInstall();
  }

  async function downloadInstaller() {
    if (!enabled || !version || busy || submitted.current || downloadPending.current) return;
    downloadPending.current = true;
    const request = ++downloadRequest.current;
    setOpeningDownload(true);
    setDownloadError(null);
    try {
      await onDownloadInstaller();
      if (request === downloadRequest.current) close();
    } catch (error) {
      if (request === downloadRequest.current) setDownloadError(error instanceof Error ? error.message : String(error));
    } finally {
      if (request === downloadRequest.current) { downloadPending.current = false; setOpeningDownload(false); }
    }
  }

  if (!open) return null;
  return <dialog ref={dialog} className={`app-update-panel is-${state.status}`} aria-labelledby={titleId}
    aria-describedby={state.status === "available" ? detailId : undefined} data-no-window-drag="true"
    onCancel={(event) => { event.preventDefault(); close(); }}
    onKeyDown={(event) => { if (dialog.current) containAssistantFocus(event, dialog.current, document.activeElement); }}>
    <header className="app-update-heading">
      <h2 ref={title} id={titleId} tabIndex={-1}>{copy[state.status]} <span className="app-update-version">{version}</span></h2>
      <button type="button" className="ghost-button app-update-close" aria-label={copy.close} onClick={close}><ShellIcon name="x" /></button>
    </header>
    <div className="app-update-body">
      {state.status === "available" ? <>
        {state.releaseNotes?.trim() ? <p className="app-update-notes">{state.releaseNotes.trim()}</p> : null}
        <p id={detailId} className="app-update-detail">{copy.installImpact}</p>
      </> : state.status === "failed" ? <p className="app-update-error" role="alert">{state.error || copy.failedDetail}</p>
        : <p className="app-update-detail">{state.status === "installing" ? copy.installingDetail
          : state.status === "downloading" ? copy.downloadingDetail : copy.checking}</p>}
      {state.status === "downloading" || state.status === "installing" ? <div className="app-update-transfer">
        <progress aria-label={copy[state.status]} max={100} value={state.status === "installing" ? undefined : percent} />
        {state.status === "downloading" && percent !== undefined ? <span>{percent}%</span> : null}
      </div> : null}
      {downloadError ? <p className="app-update-error" role="alert">{copy.downloadFailed} {downloadError}</p> : null}
    </div>
    <footer className="app-update-actions">
      <button type="button" className="ghost-button app-update-later" onClick={close}>
        {busy ? copy.dismiss : copy.later}
      </button>
      {!busy ? <>
        <button type="button" className="secondary-button" disabled={openingDownload}
          onClick={() => void downloadInstaller()}>{openingDownload ? copy.openingDownload : copy.download}</button>
        <button type="button" className="primary-button" disabled={openingDownload}
          onClick={state.status === "failed" ? onCheck : install}>{state.status === "failed" ? copy.retry : copy.install}</button>
      </> : null}
    </footer>
  </dialog>;
}
