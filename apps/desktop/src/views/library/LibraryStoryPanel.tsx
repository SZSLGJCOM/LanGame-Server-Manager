import { useEffect, useState, type MouseEvent } from "react";
import { fetchSteamStoreAbout, openExternalUrl } from "../../api";
import { LibraryStoryHtml } from "../../components/LibraryStoryHtml";
import { ActivityNotice } from "../../components/ActivityNotice";
import { useI18n } from "../../i18n";
import type { ModuleStoreEntry } from "../../store-media";
import { createLibraryDetailResourceCache } from "./library-detail-intelligence";
import { shouldUseLocalLibraryStory } from "./library-story-source";
import { sanitizeSteamAboutHtml } from "./steam-about-html";
import { readSavedLibraryStory, saveLibraryStory } from "./library-story-cache";

const storyCache = createLibraryDetailResourceCache<string | null>({
  ttlMs: 10 * 60 * 1000,
  maxEntries: 64,
  shouldCache: (html) => Boolean(html)
});

interface LibraryStoryPanelProps {
  storeEntry: ModuleStoreEntry | null;
  storyParagraphs: string[];
}

export function LibraryStoryPanel(props: LibraryStoryPanelProps) {
  const { locale } = useI18n();
  // A new game/language owns its own state. Late replies cannot replace the
  // selected story, including the first render before an effect is cleaned up.
  return <StoryContent key={`${locale}:${props.storeEntry?.storeAppId}:${props.storeEntry?.storeSource}`} {...props} />;
}

function StoryContent(props: LibraryStoryPanelProps) {
  const { locale, t } = useI18n();
  const useLocalStory = shouldUseLocalLibraryStory(locale, props.storeEntry);
  const storeAppId = !useLocalStory && props.storeEntry?.storeSource === "steam" ? props.storeEntry.storeAppId : null;
  const paragraphs = props.storeEntry?.aboutParagraphs.length ? props.storeEntry.aboutParagraphs : props.storyParagraphs;
  const [html, setHtml] = useState("");
  const [status, setStatus] = useState<"loading" | "ready" | "unavailable" | "error">(storeAppId ? "loading" : "ready");
  const [attempt, setAttempt] = useState(0);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const cacheKey = `story:${locale}:${storeAppId}`;

  useEffect(() => {
    if (!storeAppId) return;
    let cancelled = false;
    setStatus("loading");
    storyCache.read(cacheKey, async () => {
      const raw = await fetchSteamStoreAbout(storeAppId, locale);
      const sanitized = raw ? sanitizeSteamAboutHtml(raw) || null : null;
      void saveLibraryStory(cacheKey, sanitized);
      return sanitized;
    }).then(async (result) => {
      const saved = result ? null : await readSavedLibraryStory(cacheKey);
      if (cancelled) return;
      setHtml(result ?? saved?.html ?? "");
      setSavedAt(saved?.savedAt ?? null);
      setStatus(result ? "ready" : "unavailable");
    }).catch(async () => {
      const saved = await readSavedLibraryStory(cacheKey);
      if (cancelled) return;
      setHtml(saved?.html ?? "");
      setSavedAt(saved?.savedAt ?? null);
      setStatus("error");
    });
    return () => { cancelled = true; };
  }, [attempt, cacheKey, locale, storeAppId]);

  function handleHtmlClick(event: MouseEvent<HTMLElement>) {
    const link = (event.target as HTMLElement | null)?.closest("a");
    if (!link?.href) return;
    event.preventDefault();
    void openExternalUrl(link.href);
  }

  if (useLocalStory || props.storeEntry?.storeSource === "official") {
    if (!paragraphs.length) return null;
    return (
      <section className="library-story-panel library-story-panel--official-source">
        <div className="panel-head"><div>
          <span className="eyebrow">{t(useLocalStory ? "library.detail.storyEyebrow" : "library.detail.officialStoryEyebrow")}</span>
          <h3>{t(useLocalStory ? "library.detail.storyTitle" : "library.detail.officialStoryTitle")}</h3>
        </div></div>
        <div className="library-story-copy library-story-copy--official-source">
          {paragraphs.map((paragraph, index) => <p key={index}>{paragraph}</p>)}
        </div>
      </section>
    );
  }
  if (!storeAppId) return null;
  if (html) {
    return <section className="library-story-panel library-story-panel--steam-html" onClick={handleHtmlClick} aria-busy={status === "loading"}>
      {savedAt !== null ? <ActivityNotice tone="warning" action={status !== "loading"
        ? <button type="button" className="secondary-button" onClick={() => {
          storyCache.clear(cacheKey);
          setStatus("loading");
          setAttempt((value) => value + 1);
        }}>{t("common.retry")}</button> : undefined}>
        {t(status === "loading" ? "library.detail.storySavedRefreshing" : "library.detail.storySavedFallback", {
          date: new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(savedAt)
        })}
      </ActivityNotice> : null}
      <LibraryStoryHtml sanitizedHtml={html} />
    </section>;
  }
  const statusMessage = t(status === "loading" ? "library.detail.storyLoading"
    : status === "error" ? "library.detail.storyRequestFailed" : "library.detail.storyUnavailable")
    + (paragraphs.length ? ` ${t("library.detail.storyLocalFallback")}` : "");
  return (
    <section className="library-story-panel library-story-panel--steam-fallback" aria-busy={status === "loading"}>
      <div className="panel-head">
        <div><span className="eyebrow">{t("library.detail.storyEyebrow")}</span><h3>{t("library.detail.storyTitle")}</h3></div>
      </div>
      {status === "loading" && !paragraphs.length && attempt === 0 ? <p className="form-note" role="status">{statusMessage}</p>
        : <ActivityNotice tone={status === "error" ? "error" : status === "unavailable" ? "warning" : "info"}
          action={status !== "loading" ? <button type="button" className="secondary-button" onClick={() => {
          storyCache.clear(cacheKey);
          setStatus("loading");
          setAttempt((value) => value + 1);
        }}>{t("common.retry")}</button> : undefined}>{statusMessage}</ActivityNotice>}
      {paragraphs.length ? <div className="library-story-copy">
        {paragraphs.map((paragraph, index) => <p key={index}>{paragraph}</p>)}
      </div> : null}
    </section>
  );
}
