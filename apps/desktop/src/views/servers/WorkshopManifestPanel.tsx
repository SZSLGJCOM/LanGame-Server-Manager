import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { lookupSteamWorkshopItems, readSteamWorkshopInstallationStatus } from "../../api";
import { describeError } from "../../app-state";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import { inspectWorkshopManifest, parseWorkshopManifest, type WorkshopManifestReview } from "./workshop-manifest";

interface WorkshopManifestPanelProps {
  instanceId: string;
  appId: number;
  currentIds: string[];
  initialText?: string | null;
  onDraftChange: (text: string) => void;
  disabled: boolean;
  readOnly?: boolean;
  enablementNote?: string;
  targetNote?: string;
  allowCachedInstall?: boolean;
  onApply: (review: WorkshopManifestReview, enable: boolean) => Promise<boolean>;
}

export function WorkshopManifestPanel(props: WorkshopManifestPanelProps) {
  const { t, locale } = useI18n();
  const inputId = useId();
  const contextKey = JSON.stringify([props.instanceId, props.appId, locale]);
  const currentContext = useRef(contextKey);
  currentContext.current = contextKey;
  const [text, setText] = useState(() => props.initialText ?? props.currentIds.join("\n"));
  const [reviewContext, setReviewContext] = useState(contextKey);
  const [reviewResult, setReview] = useState<WorkshopManifestReview | null>(null);
  const [reviewChecking, setChecking] = useState(false);
  const [applying, setApplying] = useState(false);
  const [reviewError, setError] = useState<string | null>(null);
  const [reviewMessage, setMessage] = useState<string | null>(null);
  const review = reviewContext === contextKey ? reviewResult : null;
  const checking = reviewContext === contextKey && reviewChecking;
  const error = reviewContext === contextKey ? reviewError : null;
  const message = reviewContext === contextKey ? reviewMessage : null;
  const generation = useRef(0);
  const mounted = useRef(true);
  const parsed = useMemo(() => parseWorkshopManifest(text), [text]);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; generation.current += 1; };
  }, []);
  useEffect(() => {
    generation.current += 1;
    setReviewContext(contextKey);
    setReview(null);
    setChecking(false);
    setError(null);
    setMessage(null);
    // Installation owns its applying state until its own promise settles.
  }, [contextKey]);

  function changeText(next: string) {
    if (props.readOnly) return;
    generation.current += 1;
    setText(next);
    props.onDraftChange(next);
    setReview(null);
    setChecking(false);
    setError(null);
    setMessage(null);
  }

  async function check() {
    if (props.readOnly || parsed.tooLarge || parsed.invalidTokens.length > 0 || parsed.ids.length === 0) return;
    const revision = ++generation.current;
    const isCurrent = () => mounted.current && generation.current === revision && currentContext.current === contextKey;
    setReviewContext(contextKey);
    setChecking(true);
    setReview(null);
    setError(null);
    try {
      const result = await inspectWorkshopManifest(parsed.ids, props.appId, {
        lookup: (ids) => lookupSteamWorkshopItems(ids, locale),
        inspect: (ids) => readSteamWorkshopInstallationStatus(props.instanceId, ids),
        isCurrent
      });
      if (isCurrent()) setReview(result);
    } catch (cause) {
      if (isCurrent()) setError(describeError(cause));
    } finally {
      if (isCurrent()) setChecking(false);
    }
  }

  async function apply(enable: boolean) {
    if (props.readOnly || !review || review.issues.length > 0 || applying || props.disabled || (enable && props.enablementNote)) return;
    const revision = generation.current;
    const isCurrent = () => mounted.current && generation.current === revision && currentContext.current === contextKey;
    setApplying(true);
    setMessage(null);
    try {
      const completed = await props.onApply(review, enable);
      if (!isCurrent()) return;
      if (completed) {
        setMessage(t(enable ? "servers.mods.manifest.enabled" : "servers.mods.manifest.completed"));
        await check();
      } else {
        setReview(null);
      }
    } catch (cause) {
      if (isCurrent()) setError(describeError(cause));
    } finally {
      if (mounted.current) setApplying(false);
    }
  }

  const busy = applying || checking;
  const canCheck = !props.readOnly && !busy && parsed.ids.length > 0 && !parsed.tooLarge && parsed.invalidTokens.length === 0;
  const canApply = Boolean(!props.readOnly && review && review.contentIds.length > 0 && review.issues.length === 0 && !busy && !props.disabled);
  const installed = new Set(review?.installedIds ?? []);
  const issues = new Map(review?.issues.map((issue) => [issue.id, issue.reason]) ?? []);
  const rowIds = review ? Array.from(new Set([...review.ids, ...review.contentIds, ...issues.keys()])) : [];

  return <section className="mw-manifest" aria-busy={busy}>
    <div className="mw-manifest-input-head">
      <label htmlFor={inputId}>{t("servers.mods.manifest.input")}</label>
      <button type="button" className="mw-ghost-btn" disabled={busy || props.readOnly} onClick={() => changeText(props.currentIds.join("\n"))}>
        {t("servers.mods.manifest.useCurrent")}
      </button>
    </div>
    <textarea id={inputId} className="mw-textarea mw-manifest-input" value={text} disabled={applying} readOnly={props.readOnly}
      spellCheck={false} maxLength={1_048_577} aria-describedby={`${inputId}-help`}
      placeholder={t("servers.mods.manifest.placeholder")} onChange={(event) => changeText(event.target.value)} />
    <p id={`${inputId}-help`} className="mw-manifest-help">{t("servers.mods.manifest.help")}</p>
    {props.targetNote ? <p className="mw-manifest-help">{props.targetNote}</p> : null}
    <div className="mw-manifest-actions">
      <button type="button" className="mw-ghost-btn" disabled={!canCheck} onClick={() => void check()}>
        <ShellIcon name={checking ? "loader" : "search"} className={checking ? "mw-btn-icon mw-btn-icon--spin" : "mw-btn-icon"} />
        {t(checking ? "servers.mods.manifest.checking" : "servers.mods.manifest.check")}
      </button>
      <span className="mw-manifest-help">{t("servers.mods.manifest.parsed", { count: parsed.ids.length, duplicates: parsed.duplicateCount })}</span>
    </div>
    {parsed.tooLarge ? <div className="mw-notice mw-notice--error" role="alert">{t("servers.mods.manifest.tooLarge")}</div> : null}
    {parsed.invalidTokens.length > 0 ? <div className="mw-notice mw-notice--error" role="alert">
      {t("servers.mods.manifest.invalid", { values: parsed.invalidTokens.slice(0, 8).join("、") })}
    </div> : null}
    {error ? <ActivityNotice tone="error">{error}</ActivityNotice> : null}
    {checking ? <ActivityNotice>{t("servers.mods.manifest.checking")}</ActivityNotice> : null}
    {message ? <ActivityNotice tone="success">{message}</ActivityNotice> : null}
    {review ? <>
      <div className="mw-manifest-summary" role="status">
        {t("servers.mods.manifest.summary", { total: review.contentIds.length, installed: review.installedIds.length, missing: review.missingIds.length, blocked: review.issues.length })}
      </div>
      {review.skippedClientOnlyIds.length > 0 ? <p className="mw-manifest-help">
        {t("servers.mods.storeDetail.clientMembersSkipped", { count: review.skippedClientOnlyIds.length })}
      </p> : null}
      <div className="mw-manifest-review">
        <table className="mw-manifest-table">
          <thead><tr><th>{t("servers.mods.manifest.item")}</th><th>{t("servers.mods.manifest.status")}</th></tr></thead>
          <tbody>{rowIds.slice(0, 200).map((id) => {
            const item = review.items[id];
            const issue = issues.get(id);
            return <tr key={id}>
              <td><span>{item?.title || id}</span>{item?.title ? <small>{id}</small> : null}</td>
              <td>{issue ? t(`servers.mods.manifest.issue.${issue}`)
                : item?.item_kind === "collection" ? t("servers.mods.manifest.collection")
                : t(installed.has(id) ? "servers.mods.lifecycle.downloaded" : "servers.mods.manifest.missing")}</td>
            </tr>;
          })}</tbody>
        </table>
        {rowIds.length > 200 ? <p className="mw-manifest-help">{t("servers.mods.manifest.previewLimit", { total: rowIds.length })}</p> : null}
      </div>
      {review.issues.length > 0 ? <p className="mw-manifest-help" role="alert">{t("servers.mods.manifest.fixIssues")}</p> : null}
      {props.enablementNote ? <p className="mw-manifest-help">{props.enablementNote}</p> : null}
      <div className="mw-manifest-actions">
        <button type="button" className="mw-ghost-btn" disabled={!canApply || (review.missingIds.length === 0 && !props.allowCachedInstall)} onClick={() => void apply(false)}>
          {review.missingIds.length === 0 && props.allowCachedInstall ? t("servers.mods.manifest.installCached")
            : t("servers.mods.manifest.downloadMissing", { count: review.missingIds.length })}
        </button>
        {!props.enablementNote ? <button type="button" className="mw-btn mw-btn--primary" disabled={!canApply} onClick={() => void apply(true)}>
          {t("servers.mods.manifest.downloadEnable")}
        </button> : null}
      </div>
    </> : null}
  </section>;
}
