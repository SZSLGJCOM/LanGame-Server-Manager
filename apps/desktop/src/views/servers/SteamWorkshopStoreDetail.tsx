import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useRef } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { useMediaSource } from "../../components/useMediaSource";
import { officialMediaCandidates } from "../../official-media-sources";
import { useI18n } from "../../i18n";
import { formatDesktopError } from "../../desktop-error-message";
import { WorkshopStatus } from "./WorkshopStatus";
import { ConfigurationHelp } from "../settings/ConfigurationFieldHelp";
import { isClientOnlyDstWorkshopItem } from "./mod-workbench-model";
import type { SteamWorkshopLookupItem } from "../../types";
import {
  formatWorkshopByteSize,
  workshopDescriptionText,
  type WorkshopLifecycleState,
  type WorkshopStoreAction
} from "./steam-workshop-store-model";

export type { WorkshopStoreAction } from "./steam-workshop-store-model";

interface SteamWorkshopStoreDetailProps {
  item: SteamWorkshopLookupItem;
  lifecycleState: WorkshopLifecycleState;
  action: WorkshopStoreAction;
  actionBusy: boolean;
  actionDisabled: boolean;
  progressPercent?: number | null;
  detailsReady: boolean;
  detailsError?: string | null;
  onRetryDetails?: () => void;
  onAction: () => void;
  onOpenChild: (childId: string) => void;
  onBack?: () => void;
  backLabel?: string;
  onClose: () => void;
  onOpenExternal: () => void;
}

export function SteamWorkshopPreview({ url, loading = false }: { url: string | null | undefined; loading?: boolean }) {
  const image = useMediaSource(officialMediaCandidates(url, "image"));
  return image.src
    ? <img key={image.src} ref={image.ref} src={image.src} alt="" loading={loading ? "lazy" : undefined}
        referrerPolicy="no-referrer" onError={image.onError} onLoad={image.onLoad} />
    : <span className="mw-mod-thumb-placeholder">MOD</span>;
}

function formatDate(locale: string, value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) {
    return "";
  }
  return new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "short",
    day: "numeric"
  }).format(new Date(value * 1000));
}

function formatCount(locale: string, value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value)
    ? new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: 1 }).format(value)
    : "";
}

export function SteamWorkshopStoreDetail(props: SteamWorkshopStoreDetailProps) {
  const { locale, t } = useI18n();
  const heading = useRef<HTMLHeadingElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (scroll.current) scroll.current.scrollTop = 0;
    heading.current?.focus({ preventScroll: true });
  }, [props.item.id]);
  const description = workshopDescriptionText(props.item.description);
  const metrics = [
    { id: "fileSize", icon: "hard-drive" as const, label: t("servers.mods.storeDetail.fileSize", undefined, "File size"), value: formatWorkshopByteSize(props.item.file_size) },
    { id: "updated", icon: "refresh" as const, label: t("servers.mods.storeDetail.updated", undefined, "Updated"), value: formatDate(locale, props.item.updated_at_unix) },
    { id: "created", icon: "clock" as const, label: t("servers.mods.storeDetail.created", undefined, "Created"), value: formatDate(locale, props.item.created_at_unix) },
    { id: "subscribers", icon: "users" as const, label: t("servers.mods.storeDetail.subscribers", undefined, "Subscribers"), value: formatCount(locale, props.item.subscriptions) },
    { id: "favorites", icon: "star" as const, label: t("servers.mods.storeDetail.favorites", undefined, "Favorites"), value: formatCount(locale, props.item.favorites) },
    { id: "views", icon: "eye" as const, label: t("servers.mods.storeDetail.views", undefined, "Views"), value: formatCount(locale, props.item.views) }
  ].filter((metric) => metric.value);
  const actionLabel = props.actionBusy
    ? t("servers.mods.installRunning", undefined, "Installing…")
    : props.action === "manage"
      ? t("servers.mods.storeDetail.manage", undefined, "Manage")
      : t("servers.mods.storeDetail.install", undefined, "Download and add");
  const actionIcon = props.actionBusy
    ? "loader"
    : props.action === "manage"
      ? "settings"
      : "plus";
  const steamLabel = t("servers.mods.storeDetail.openSteam", undefined, "Open in Steam");
  const tags = props.item.tags ?? [];
  const tagsLabel = tags.length > 0
    ? `${t("servers.mods.storeDetail.tags", undefined, "Tags")} ${tags.join("、")}`
    : "";
  const title = props.item.title ?? t("servers.mods.workshopItemFallback", { id: props.item.id }, "Workshop item {id}");
  const children = props.item.children.filter((child) => !(child.status === "resolved" && child.item_kind === "item" &&
    child.consumer_app_id === props.item.consumer_app_id && isClientOnlyDstWorkshopItem(child, props.item.consumer_app_id ?? null)));
  const skippedChildren = props.item.children.length - children.length;

  return (
    <aside className="mw-store-detail" aria-busy={!props.detailsReady && !props.detailsError}
      aria-label={t("servers.mods.storeDetail.title", undefined, "Workshop item details")}>
      <div className="mw-store-detail-head">
        <div className="mw-store-detail-heading">
          {props.onBack ? <button type="button" className="mw-ghost-btn mw-store-detail-back" onClick={props.onBack}>
            <ShellIcon name="chevron-left" className="mw-btn-icon" />
            {props.backLabel || t("servers.mods.storeDetail.backToCollection", undefined, "Back to collection")}
          </button> : null}
          <h2 ref={heading} tabIndex={-1}>{title}</h2>
          <span className="mw-store-detail-meta">
            <span className="mw-store-detail-id">{props.item.id}</span>
            {props.detailsReady && props.lifecycleState === "client-only" ? <WorkshopStatus state="client-only" /> : null}
          </span>
        </div>
        <button
          type="button"
          className="mw-store-detail-close"
          aria-label={t("servers.mods.storeDetail.close", undefined, "Close details")}
          onClick={props.onClose}
        >
          <ShellIcon name="x" className="mw-store-detail-close-icon" />
        </button>
      </div>

      <div ref={scroll} className={`mw-store-detail-scroll${props.detailsReady ? "" : " mw-store-detail-scroll--pending"}`}>
        {!props.detailsReady ? props.detailsError ? (
          <div className="mw-empty mw-store-detail-error" role="alert">
            <ShellIcon name="alert-triangle" className="mw-empty-icon" />
            <span>{props.detailsError}</span>
            <div className="mw-store-detail-error-actions">
              {props.onRetryDetails ? <button type="button" className="mw-ghost-btn" onClick={props.onRetryDetails}>
                {t("common.retry", undefined, "Retry")}
              </button> : null}
              <button type="button" className="mw-ghost-btn" onClick={props.onOpenExternal}>{steamLabel}</button>
            </div>
          </div>
        ) : (
          <div className="mw-empty mw-store-detail-loading" role="status">
            <ShellIcon name="loader" className="mw-empty-icon mw-empty-icon--spin" />
            <span>{t("servers.mods.detailLoading", undefined, "Loading Mod details…")}</span>
          </div>
        ) : <>
        {props.detailsError ? <ActivityNotice tone="error" action={<button type="button" className="mw-ghost-btn" onClick={props.onRetryDetails}>{t("common.retry", undefined, "Retry")}</button>}>{props.detailsError}</ActivityNotice> : null}
        {props.item.localization_warning ? <ActivityNotice tone="warning" action={props.onRetryDetails ?
          <button type="button" className="mw-ghost-btn" onClick={props.onRetryDetails}>{t("common.retry", undefined, "Retry")}</button> : undefined}>
          {`${t("servers.mods.storeDetail.originalTextFallback", undefined, "Workshop details could not be loaded in the selected language. Showing the author's original text; retry to load localized content.")} ${formatDesktopError(t, props.item.localization_warning)}`}
        </ActivityNotice> : null}
        <div className="mw-store-detail-preview">
          <SteamWorkshopPreview url={props.item.preview_url} />
        </div>

        {props.lifecycleState === "client-only" ? null : (
          <div className="mw-store-detail-status">
            <WorkshopStatus state={props.lifecycleState} />
          </div>
        )}

        <div className="mw-store-detail-metrics">
          <ConfigurationHelp description={actionLabel}>{(help) => <span
            className="mw-store-detail-metric-help" ref={help.anchorRef} {...help.interactionProps}
            tabIndex={props.actionDisabled ? 0 : undefined}
            role={props.actionDisabled ? "group" : undefined}
            aria-disabled={props.actionDisabled || undefined}
            aria-label={props.actionDisabled ? actionLabel : undefined}
            aria-describedby={props.actionDisabled ? help.descriptionId : undefined}>
            <button
              type="button"
              className="mw-store-detail-metric"
              disabled={props.actionDisabled}
              aria-label={actionLabel}
              aria-describedby={help.descriptionId}
              onClick={props.onAction}
            >
              <ShellIcon
                name={actionIcon}
                className={props.actionBusy ? "mw-store-detail-metric-icon mw-btn-icon--spin" : "mw-store-detail-metric-icon"}
              />
            </button>
          </span>}</ConfigurationHelp>
          <ConfigurationHelp description={steamLabel}>{(help) => <button
            type="button"
            className="mw-store-detail-metric"
            aria-label={steamLabel}
            ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
            onClick={props.onOpenExternal}
          >
            <ShellIcon name="external-link" className="mw-store-detail-metric-icon" />
          </button>}</ConfigurationHelp>
          {metrics.length > 0 || tagsLabel ? <span className="mw-store-detail-metric-gap" aria-hidden="true" /> : null}
          {metrics.map((metric) => (
            <ConfigurationHelp key={metric.id} description={`${metric.label} ${metric.value}`}>{(help) => <span
              className="mw-store-detail-metric"
              tabIndex={0}
              aria-label={`${metric.label} ${metric.value}`}
              ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
            >
              <ShellIcon name={metric.icon} className="mw-store-detail-metric-icon" />
            </span>}</ConfigurationHelp>
          ))}
          {tagsLabel ? (
            <ConfigurationHelp description={tagsLabel}>{(help) => <span
              className="mw-store-detail-metric"
              tabIndex={0}
              aria-label={tagsLabel}
              ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
            >
              <ShellIcon name="tag" className="mw-store-detail-metric-icon" />
            </span>}</ConfigurationHelp>
          ) : null}
        </div>

        {props.actionBusy || props.lifecycleState === "installing" ? (
          <div className="mw-store-detail-progress" role="progressbar" aria-label={t("servers.mods.installRunning", undefined, "Installing…")} aria-valuenow={props.progressPercent ?? undefined}>
            <div
              className={props.progressPercent !== null && props.progressPercent !== undefined
                ? "mw-store-detail-progress-bar mw-store-detail-progress-bar--determinate"
                : "mw-store-detail-progress-bar"}
              style={props.progressPercent !== null && props.progressPercent !== undefined
                ? { width: `${Math.max(6, Math.min(100, props.progressPercent))}%` }
                : undefined}
            />
          </div>
        ) : null}

        <section className="mw-store-detail-section">
          <h3>{t("servers.mods.storeDetail.description", undefined, "Description")}</h3>
          <p className="mw-store-detail-description">
            {description || t("servers.mods.storeDetail.noDescription", undefined, "No description is available for this item.")}
          </p>
        </section>

        {props.item.children.length > 0 ? (
          <section className="mw-store-detail-section">
            <h3>
              {t("servers.mods.storeDetail.collectionItems", { count: children.length }, "Collection items ({count})")}
            </h3>
            {skippedChildren ? <p className="mw-store-detail-children-note">{t("servers.mods.storeDetail.clientMembersSkipped", { count: skippedChildren },
              "{count} client-only Mods skipped")}</p> : null}
            {children.length ? <div className="mw-store-detail-children">
              {children.map((child) => {
                const childTitle = child.title ?? t("servers.mods.workshopItemFallback", { id: child.id }, "Workshop item {id}");
                return <button key={child.id} type="button" className="mw-store-detail-child"
                  aria-label={t("servers.mods.storeDetail.openDetails", { name: childTitle }, "View details for {name}")}
                  onClick={() => props.onOpenChild(child.id)}>
                  <span>{childTitle}</span>
                  <small>{child.id}</small>
                  <ShellIcon name="chevron-right" className="mw-btn-icon" />
                </button>;
              })}
            </div> : <div className="mw-empty mw-store-detail-children-empty">
              <ShellIcon name="package" className="mw-empty-icon" />
              <span>{t("servers.mods.storeDetail.noServerMembers", undefined, "No server Mods are available in this collection.")}</span>
            </div>}
          </section>
        ) : null}
        </>}
      </div>
    </aside>
  );
}
