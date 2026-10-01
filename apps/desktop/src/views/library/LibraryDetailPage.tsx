import { useEffect, useRef, useState, type ReactNode } from "react";
import { fetchSteamReviewSummary, openExternalUrl } from "../../api";
import { useI18n } from "../../i18n";
import { ModuleCover } from "../../components/ModuleCover";
import { StreamVideo } from "../../components/StreamVideo";
import { LibraryStoryPanel } from "./LibraryStoryPanel";
import { LibraryUpdatesPanel } from "./LibraryUpdatesPanel";
import { ShellIcon } from "../../components/ShellIcon";
import { LibraryServerActions } from "./LibraryServerActions";
import { resolveModulePresentationFallbackCoverSrc } from "../../module-presentation";
import type { ModuleMediaItem, ModuleStoreEntry } from "../../store-media";
import type { CreateInstanceInput, ModuleDetails, ModuleSummary, SteamCmdStatus, SteamReviewSummary } from "../../types";
import {
  platformLabel,
  type LibraryInstallTone
} from "./library-shared";
import {
  buildLibraryDetailProfile,
  createLibraryDetailResourceCache,
  formatSteamReviewSummary,
  shouldFetchSteamReviewSummary
} from "./library-detail-intelligence";

const LIBRARY_DETAIL_RESOURCE_CACHE_TTL_MS = 10 * 60 * 1000;
const steamReviewSummaryResourceCache = createLibraryDetailResourceCache<SteamReviewSummary | null>({
  ttlMs: LIBRARY_DETAIL_RESOURCE_CACHE_TTL_MS
});

interface LibraryDetailPageProps {
  selected: ModuleSummary | null;
  selectedModuleDetails: ModuleDetails | null;
  steamCmdStatus: SteamCmdStatus | null;
  steamCmdBusy: boolean;
  storeEntry: ModuleStoreEntry | null;
  mediaItems: ModuleMediaItem[];
  activeMedia: ModuleMediaItem | null;
  heroTitle: string;
  heroSubtitle: string;
  detailHeaderMeta: string;
  storyParagraphs: string[];
  categoryTags: string[];
  genreTags: string[];
  installLabel: string;
  installBusy: boolean;
  installStatusClass: LibraryInstallTone;
  creating: boolean;
  creationStartedAt?: number;
  instanceName: string;
  onBackToCatalog: () => void;
  onActiveMediaChange: (key: string) => void;
  onInstall: (moduleId: string, validate: boolean) => void;
  onUninstall: (moduleId: string) => void | Promise<void>;
  onCreateServer: (input: CreateInstanceInput) => Promise<void>;
  onInstanceNameChange: (value: string) => void;
}

function findLibraryScrollContainer() {
  if (typeof document === "undefined") {
    return null;
  }

  return document.querySelector<HTMLElement>(".shell-content-scroll");
}

interface LibraryDetailHeaderProps {
  heroTitle: string;
  meta: string;
  onBackToCatalog: () => void;
}

function LibraryDetailHeader(props: LibraryDetailHeaderProps) {
  const { t } = useI18n();

  return (
    <section className="workspace-header library-detail-header">
      <div className="library-detail-header-bg" aria-hidden="true">
        <div className="library-detail-header-gradient" />
      </div>
      <div className="workspace-title-group">
        <h1 className="workspace-title">{props.heroTitle}</h1>
        {props.meta ? <p className="library-detail-header-meta">{props.meta}</p> : null}
      </div>
      <div className="workspace-header-actions">
        <button type="button" className="library-back-button" onClick={props.onBackToCatalog}>
          <ShellIcon name="chevron-left" className="shell-small-icon" />
          <span>{t("library.detail.backToCatalog")}</span>
        </button>
      </div>
    </section>
  );
}

interface LibraryDetailMediaStageProps {
  moduleId: string;
  heroTitle: string;
  mediaItems: ModuleMediaItem[];
  activeMedia: ModuleMediaItem | null;
  onActiveMediaChange: (key: string) => void;
}

function LibraryTrailerPlayer({
  moduleId,
  heroTitle,
  media
}: {
  moduleId: string;
  heroTitle: string;
  media: ModuleMediaItem;
}) {
  const [videoReady, setVideoReady] = useState(false);
  const [videoFallbackToCover, setVideoFallbackToCover] = useState(false);
  const fallbackCoverSrc = resolveModulePresentationFallbackCoverSrc(moduleId);

  useEffect(() => {
    setVideoReady(false);
    setVideoFallbackToCover(false);
  }, [media.key, media.streamUrl]);

  return (
    <div className="library-media-player library-media-video-surface">
      <ModuleCover
        moduleId={moduleId}
        moduleName={heroTitle}
        subtitle=""
        size="hero"
        showOverlay={false}
        imageSrc={videoFallbackToCover ? fallbackCoverSrc : media.imageSrc}
        imageAlt={`${heroTitle} ${media.title}`}
        className="library-media-video-poster"
      />
      {media.streamUrl ? (
        <StreamVideo
          key={media.key}
          streamUrl={media.streamUrl}
          className={videoReady ? "library-media-video is-ready" : "library-media-video"}
          controls
          playsInline
          preload="metadata"
          aria-label={`${heroTitle} ${media.title}`}
          onCanPlay={() => setVideoReady(true)}
          onLoadedData={() => setVideoReady(true)}
          onPlaying={() => setVideoReady(true)}
          onError={() => {
            setVideoReady(false);
            setVideoFallbackToCover(true);
          }}
        />
      ) : null}
    </div>
  );
}

function LibraryDetailMediaStage(props: LibraryDetailMediaStageProps) {
  const { t } = useI18n();

  return (
    <article className="panel-card library-detail-stage-media">
      <div className="library-detail-stage-player">
        {props.activeMedia ? (
          <>
            {props.activeMedia.kind === "trailer" && props.activeMedia.streamUrl ? (
              <LibraryTrailerPlayer
                moduleId={props.moduleId}
                heroTitle={props.heroTitle}
                media={props.activeMedia}
              />
            ) : props.activeMedia.imageSrc ? (
              <ModuleCover
                moduleId={props.moduleId}
                moduleName={props.heroTitle}
                subtitle=""
                size="hero"
                showOverlay={false}
                imageSrc={props.activeMedia.imageSrc}
                imageAlt={`${props.heroTitle} ${props.activeMedia.title}`}
                className="library-media-player library-media-resilient-cover"
              />
            ) : (
              <div className="library-media-fallback">{t("library.detail.noMediaSynced")}</div>
            )}
          </>
        ) : (
          <div className="library-media-fallback">{t("library.detail.noGallerySynced")}</div>
        )}
      </div>

      {props.mediaItems.length ? (
        <div className="library-media-steam-rail">
          {props.mediaItems.map((item) => {
            const active = item.key === props.activeMedia?.key;
            return (
              <button
                key={item.key}
                type="button"
                className={active ? "library-media-steam-thumb is-active" : "library-media-steam-thumb"}
                onClick={() => props.onActiveMediaChange(item.key)}
                aria-label={item.title}
              >
                <ModuleCover
                  moduleId={props.moduleId}
                  moduleName={props.heroTitle}
                  subtitle=""
                  showOverlay={false}
                  imageSrc={item.thumbnailSrc}
                  className="library-media-steam-thumb-image"
                />
                {item.kind === "trailer" ? <div className="video-overlay" /> : null}
              </button>
            );
          })}
        </div>
      ) : null}
    </article>
  );
}

interface LibraryCoverSidebarProps {
  selected: ModuleSummary;
  selectedModuleDetails: ModuleDetails | null;
  steamCmdStatus: SteamCmdStatus | null;
  steamCmdBusy: boolean;
  storeEntry: ModuleStoreEntry | null;
  genreTags: string[];
  categoryTags: string[];
  installLabel: string;
  installBusy: boolean;
  installStatusClass: LibraryInstallTone;
  creating: boolean;
  creationStartedAt?: number;
  instanceName: string;
  onInstall: (moduleId: string, validate: boolean) => void;
  onUninstall: (moduleId: string) => void | Promise<void>;
  onCreateServer: (input: CreateInstanceInput) => Promise<void>;
  onInstanceNameChange: (value: string) => void;
}

function SidebarFact({ label, value, isCode }: { label: string; value: ReactNode; isCode?: boolean }) {
  if (!value) return null;
  return (
    <div className="library-sidebar-fact-row">
      <span className="library-sidebar-fact-label">{label}</span>
      <span className={`library-sidebar-fact-value ${isCode ? "is-code" : ""}`}>{value}</span>
    </div>
  );
}

function LibraryCoverSidebar(props: LibraryCoverSidebarProps) {
  const { t } = useI18n();
  const allTags = Array.from(new Set([...props.genreTags, ...props.categoryTags]));
  const storeLinkLabel = props.storeEntry?.storeSource === "official"
    ? t("library.detail.officialSiteLink")
    : t("library.detail.storeLink");

  return (
    <div className="library-cover-sidebar">
      <article className="panel-card library-sidebar-summary">
        <div className="library-sidebar-info">
          <ModuleCover
            moduleId={props.selected.id}
            moduleName={props.selected.name}
            subtitle=""
            showOverlay={false}
            imageLoading="eager"
            imageSrc={props.storeEntry?.coverUrl}
            imageAlt={props.storeEntry?.storeName || props.selected.name}
            className="library-sidebar-cover"
          />
          <div className="library-sidebar-specs">
            <SidebarFact label={t("library.detail.releaseDate")} value={props.storeEntry?.releaseDate || t("library.detail.pendingSync")} />
            <SidebarFact label={t("library.detail.developer")} value={props.storeEntry?.developers[0] || t("library.detail.pendingSync")} />
            <SidebarFact label={t("library.detail.publisher")} value={props.storeEntry?.publishers[0] || t("library.detail.pendingSync")} />
            <SidebarFact label={t("library.detail.platforms")} value={platformLabel(props.selected.supported_platforms, t)} />
          </div>

          {allTags.length ? (
            <div className="library-sidebar-tags">
              <div className="page-chip-row page-chip-row--wrap">
                {allTags.map((tag) => (
                  <span key={tag} className="page-chip">{tag}</span>
                ))}
              </div>
            </div>
          ) : null}

          {props.storeEntry?.storeUrl ? (
            <a
              className="library-detail-store-link"
              href={props.storeEntry.storeUrl}
              target="_blank"
              rel="noreferrer"
              onClick={(event) => {
                event.preventDefault();
                void openExternalUrl(props.storeEntry!.storeUrl);
              }}
            >
              {storeLinkLabel}
            </a>
          ) : null}
        </div>

        <LibraryServerActions
          selected={props.selected}
          selectedModuleDetails={props.selectedModuleDetails}
          steamCmdStatus={props.steamCmdStatus}
          steamCmdBusy={props.steamCmdBusy}
          installLabel={props.installLabel}
          installBusy={props.installBusy}
          installStatusClass={props.installStatusClass}
          creating={props.creating}
          creationStartedAt={props.creationStartedAt}
          instanceName={props.instanceName}
          onInstall={props.onInstall}
          onUninstall={props.onUninstall}
          onCreateServer={props.onCreateServer}
          onInstanceNameChange={props.onInstanceNameChange}
        />
      </article>
    </div>
  );
}

function LibraryReviewSummaryPanel({ moduleId, storeEntry }: { moduleId: string; storeEntry: ModuleStoreEntry | null }) {
  const { locale, t } = useI18n();
  const profile = buildLibraryDetailProfile(moduleId, storeEntry);
  const storeAppId = shouldFetchSteamReviewSummary(profile) ? profile.steamAppId : null;
  const [summary, setSummary] = useState<SteamReviewSummary | null>(null);
  const [loading, setLoading] = useState(Boolean(storeAppId));
  const [error, setError] = useState(false);

  useEffect(() => {
    let cancelled = false;
    if (!storeAppId) {
      setSummary(null);
      setLoading(false);
      setError(false);
      return () => {
        cancelled = true;
      };
    }

    setSummary(null);
    setLoading(true);
    setError(false);
    steamReviewSummaryResourceCache.read(`review:${locale}:${storeAppId}`, () => fetchSteamReviewSummary(storeAppId, locale))
      .then((reviewSummary) => {
        if (cancelled) {
          return;
        }
        setSummary(reviewSummary);
      })
      .catch(() => {
        if (cancelled) {
          return;
        }
        setError(true);
      })
      .finally(() => {
        if (cancelled) {
          return;
        }
        setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [locale, storeAppId]);

  if (!storeAppId) {
    return null;
  }

  const formatted = summary ? formatSteamReviewSummary(summary, locale) : null;

  return (
    <section className="panel-card library-review-summary-panel">
      <div className="library-review-summary-head">
        <div className="eyebrow">{t("library.detail.reviewsEyebrow")}</div>
        <h3>{t("library.detail.reviewsTitle")}</h3>
      </div>
      <div className="library-review-summary-content">
        {loading ? <div className="form-note">{t("library.detail.reviewsLoading")}</div> : null}
        {!loading && error ? <div className="form-note">{t("library.detail.reviewsError")}</div> : null}
        {!loading && !error && !formatted ? <div className="form-note">{t("library.detail.reviewsUnavailable")}</div> : null}
        {!loading && !error && formatted ? (
          <>
            <div className="library-review-score">
              <strong>{formatted.scoreLabel}</strong>
              <span>{t("library.detail.reviewsPositive", { percent: formatted.positivePercentLabel }, "{percent} positive")}</span>
            </div>
            <div className="library-review-meta">
              <span>{t("library.detail.reviewsTotal", { count: formatted.totalReviewsLabel }, "{count} reviews")}</span>
              <a
                href={formatted.sourceUrl}
                target="_blank"
                rel="noreferrer"
                onClick={(event) => {
                  event.preventDefault();
                  void openExternalUrl(formatted.sourceUrl);
                }}
              >
                {t("library.detail.reviewsOpen")}
              </a>
            </div>
          </>
        ) : null}
      </div>
    </section>
  );
}

export function LibraryDetailPage(props: LibraryDetailPageProps) {
  const { t } = useI18n();
  const selectedModuleId = props.selected?.id ?? "";
  const scrollSnapshotRef = useRef({ moduleId: "", topByModule: new Map<string, number>() });

  useEffect(() => {
    const container = findLibraryScrollContainer();
    if (!container) {
      return;
    }

    const moduleId = selectedModuleId;
    if (!moduleId) {
      return;
    }

    const onScroll = () => {
      scrollSnapshotRef.current.topByModule.set(moduleId, container.scrollTop);
    };

    container.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      container.removeEventListener("scroll", onScroll);
    };
  }, [selectedModuleId]);

  useEffect(() => {
    const container = findLibraryScrollContainer();
    if (!container) {
      return;
    }

    if (selectedModuleId && scrollSnapshotRef.current.moduleId !== selectedModuleId) {
      scrollSnapshotRef.current.moduleId = selectedModuleId;
      const nextTop = scrollSnapshotRef.current.topByModule.get(selectedModuleId);
      if (nextTop !== undefined) {
        container.scrollTop = nextTop;
      }
    }
  }, [selectedModuleId]);

  if (!props.selected) {
    return (
      <div className="page-grid workspace-page library-detail-page library-detail-page--immersive">
        <section className="panel-card panel-card--centered">
          <div className="empty-state">{t("library.detail.empty")}</div>
        </section>
      </div>
    );
  }

  return (
    <div className="page-grid workspace-page library-detail-page library-detail-page--immersive">
      <LibraryDetailHeader
        heroTitle={props.heroTitle}
        meta={props.detailHeaderMeta}
        onBackToCatalog={props.onBackToCatalog}
      />

      <section className="library-detail-steam-layout">
        <div className="steam-layout-left">
          <LibraryDetailMediaStage
            moduleId={props.selected.id}
            heroTitle={props.heroTitle}
            mediaItems={props.mediaItems}
            activeMedia={props.activeMedia}
            onActiveMediaChange={props.onActiveMediaChange}
          />
        </div>

        <div className="steam-layout-right">
          <LibraryCoverSidebar
            selected={props.selected}
            selectedModuleDetails={props.selectedModuleDetails}
            steamCmdStatus={props.steamCmdStatus}
            steamCmdBusy={props.steamCmdBusy}
            storeEntry={props.storeEntry}
            genreTags={props.genreTags}
            categoryTags={props.categoryTags}
            installLabel={props.installLabel}
            installBusy={props.installBusy}
            installStatusClass={props.installStatusClass}
            creating={props.creating}
            creationStartedAt={props.creationStartedAt}
            instanceName={props.instanceName}
            onInstall={props.onInstall}
            onUninstall={props.onUninstall}
            onCreateServer={props.onCreateServer}
            onInstanceNameChange={props.onInstanceNameChange}
          />
        </div>
      </section>

      <LibraryStoryPanel storeEntry={props.storeEntry} storyParagraphs={props.storyParagraphs} />
      <LibraryReviewSummaryPanel moduleId={props.selected.id} storeEntry={props.storeEntry} />
      <LibraryUpdatesPanel storeEntry={props.storeEntry} />
    </div>
  );
}
