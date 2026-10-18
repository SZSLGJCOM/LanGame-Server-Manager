import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { lookupSteamWorkshopItems, readSteamWorkshopInstallationStatus } from "../../api";
import { describeError } from "../../app-state";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import { useConfigurationFieldHelp } from "../settings/ConfigurationFieldHelp";
import { ManifestHelpTrigger } from "./ManifestHelpTrigger";
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
  const inputInvalid = parsed.tooLarge || parsed.invalidTokens.length > 0;
  // Long explanations live in the shared help bubble; the text stays in the accessibility tree via aria-describedby.
  const help = useConfigurationFieldHelp(`${inputId}-help`, [t("servers.mods.manifest.help"), props.targetNote].filter(Boolean).join("\n\n"),
    undefined, undefined, "instructions");
  const downloadOnlyHelp = useConfigurationFieldHelp(`${inputId}-download-only`, props.enablementNote ? null : t("servers.mods.manifest.downloadOnlyHelp"),
    undefined, undefined, "instructions");
  const reviewHeadingId = `${inputId}-review`;
  const rowStatus = (id: string, item: WorkshopManifestReview["items"][string] | undefined) => {
    const issue = issues.get(id);
    if (issue) return { tone: "mw-chip--warn", label: t(`servers.mods.manifest.issue.${issue}`) };
    if (item?.item_kind === "collection") return { tone: "", label: t("servers.mods.manifest.collection") };
    return installed.has(id)
      ? { tone: "mw-chip--installation-installed", label: t("servers.mods.lifecycle.downloaded") }
      : { tone: "mw-chip--installation-not-installed", label: t("servers.mods.manifest.missing") };
  };

  return <section className="mw-manifest" aria-busy={busy}>
    <div className="mw-manifest-pane mw-manifest-pane--input">
      <div className="mw-manifest-pane-head mw-manifest-input-head">
        <span className="mw-manifest-pane-title">
          <label htmlFor={inputId}>{t("servers.mods.manifest.input")}</label>
          <ManifestHelpTrigger binding={help} label={t("servers.mods.manifest.helpLabel")} />
        </span>
        <button type="button" className="mw-ghost-btn" disabled={busy || props.readOnly} onClick={() => changeText(props.currentIds.join("\n"))}>
          {t("servers.mods.manifest.useCurrent")}
        </button>
      </div>
      <div className="mw-manifest-pane-body">
        <textarea id={inputId} className="mw-textarea mw-manifest-input" value={text} disabled={applying} readOnly={props.readOnly}
          spellCheck={false} maxLength={1_048_577} aria-describedby={help.descriptionId} aria-invalid={inputInvalid || undefined}
          placeholder={t("servers.mods.manifest.placeholder")} onChange={(event) => changeText(event.target.value)} />
        {parsed.tooLarge ? <div className="mw-notice mw-notice--error" role="alert">{t("servers.mods.manifest.tooLarge")}</div> : null}
        {parsed.invalidTokens.length > 0 ? <div className="mw-notice mw-notice--error" role="alert">
          {t("servers.mods.manifest.invalid", { values: parsed.invalidTokens.slice(0, 8).join("、") })}
        </div> : null}
      </div>
      <div className="mw-manifest-toolbar">
        <button type="button" className="mw-btn mw-btn--secondary" disabled={!canCheck} onClick={() => void check()}>
          <ShellIcon name={checking ? "loader" : "search"} className={checking ? "mw-btn-icon mw-btn-icon--spin" : "mw-btn-icon"} />
          {t(checking ? "servers.mods.manifest.checking" : "servers.mods.manifest.check")}
        </button>
        <span className="mw-manifest-meta">{t("servers.mods.manifest.parsed", { count: parsed.ids.length, duplicates: parsed.duplicateCount })}</span>
      </div>
    </div>
    <div className="mw-manifest-pane mw-manifest-pane--review" role="region" aria-labelledby={reviewHeadingId}>
      <div className="mw-manifest-pane-head">
        <span id={reviewHeadingId} className="mw-manifest-pane-title">{t("servers.mods.manifest.status")}</span>
        {review ? <span className="mw-manifest-summary" role="status">
          {t("servers.mods.manifest.summary", { total: review.contentIds.length, installed: review.installedIds.length, missing: review.missingIds.length, blocked: review.issues.length })}
        </span> : null}
      </div>
      <div className="mw-manifest-pane-body">
        {error ? <ActivityNotice tone="error">{error}</ActivityNotice> : null}
        {message ? <ActivityNotice tone="success">{message}</ActivityNotice> : null}
        {review ? <>
          {review.skippedClientOnlyIds.length > 0 ? <p className="mw-manifest-note">
            {t("servers.mods.storeDetail.clientMembersSkipped", { count: review.skippedClientOnlyIds.length })}
          </p> : null}
          <div className="mw-manifest-review">
            <table className="mw-manifest-table">
              <thead><tr><th scope="col">{t("servers.mods.manifest.item")}</th><th scope="col">{t("servers.mods.manifest.status")}</th></tr></thead>
              <tbody>{rowIds.slice(0, 200).map((id) => {
                const item = review.items[id];
                const status = rowStatus(id, item);
                return <tr key={id}>
                  <td><span>{item?.title || id}</span>{item?.title ? <small>{id}</small> : null}</td>
                  <td><span className={status.tone ? `mw-chip ${status.tone}` : "mw-chip"}>{status.label}</span></td>
                </tr>;
              })}</tbody>
            </table>
            {rowIds.length > 200 ? <p className="mw-manifest-note">{t("servers.mods.manifest.previewLimit", { total: rowIds.length })}</p> : null}
          </div>
        </> : <div className="mw-manifest-empty" role="status">
          {checking ? <ShellIcon name="loader" className="mw-btn-icon mw-btn-icon--spin" /> : <ShellIcon name="list" className="mw-btn-icon" />}
          <span>{t(checking ? "servers.mods.manifest.checking" : "servers.mods.manifest.reviewEmpty")}</span>
        </div>}
      </div>
      {review ? <div className="mw-manifest-actions">
        {review.issues.length > 0 ? <p className="mw-manifest-note mw-manifest-note--error" role="alert">{t("servers.mods.manifest.fixIssues")}</p> : null}
        {props.enablementNote ? <p className="mw-manifest-note">{props.enablementNote}</p> : null}
        <div className="mw-manifest-action-buttons">
          <span className="mw-manifest-action-help" ref={downloadOnlyHelp.anchorRef} {...downloadOnlyHelp.interactionProps}>
            <button type="button" className="mw-btn mw-btn--secondary" aria-describedby={downloadOnlyHelp.descriptionId}
              disabled={!canApply || (review.missingIds.length === 0 && !props.allowCachedInstall)} onClick={() => void apply(false)}>
              {review.missingIds.length === 0 && props.allowCachedInstall ? t("servers.mods.manifest.installCached")
                : t("servers.mods.manifest.downloadMissing", { count: review.missingIds.length })}
            </button>
          </span>
          {downloadOnlyHelp.helpNode}
          {!props.enablementNote ? <button type="button" className="mw-btn mw-btn--primary" disabled={!canApply} onClick={() => void apply(true)}>
            {t("servers.mods.manifest.downloadEnable")}
          </button> : null}
        </div>
      </div> : null}
    </div>
  </section>;
}
