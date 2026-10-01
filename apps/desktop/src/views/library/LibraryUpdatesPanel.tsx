import { useEffect, useMemo, useState } from "react";
import { fetchSteamNewsForApp, openExternalUrl } from "../../api";
import { useI18n } from "../../i18n";
import { ActivityNotice } from "../../components/ActivityNotice";
import type { ModuleStoreEntry } from "../../store-media";
import type { SteamNewsItem } from "../../types";
import { createLibraryDetailResourceCache, formatSteamNewsDigest } from "./library-detail-intelligence";

const STEAM_NEWS_FETCH_COUNT = 6;
const STEAM_NEWS_DISPLAY_COUNT = 3;
const steamNewsResourceCache = createLibraryDetailResourceCache<SteamNewsItem[]>({
  ttlMs: 10 * 60 * 1000,
  maxEntries: 64
});

export function LibraryUpdatesPanel({ storeEntry }: { storeEntry: ModuleStoreEntry | null }) {
  const { locale } = useI18n();
  return <UpdatesContent key={`${locale}:${storeEntry?.storeAppId}:${storeEntry?.storeSource}`} storeEntry={storeEntry} />;
}

function UpdatesContent({ storeEntry }: { storeEntry: ModuleStoreEntry | null }) {
  const { locale, t } = useI18n();
  const storeAppId = storeEntry?.storeSource === "steam" ? storeEntry.storeAppId : null;
  const officialLinks = storeEntry?.storeSource === "official" ? storeEntry.officialLinks : [];
  const [items, setItems] = useState<SteamNewsItem[]>([]);
  const [loading, setLoading] = useState(Boolean(storeAppId));
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const cacheKey = `news:${locale}:${storeAppId}:${STEAM_NEWS_FETCH_COUNT}`;
  const displayItems = useMemo(() => formatSteamNewsDigest(items, locale, STEAM_NEWS_DISPLAY_COUNT), [items, locale]);

  useEffect(() => {
    let cancelled = false;
    if (!storeAppId) {
      setItems([]);
      setLoading(false);
      setError(false);
      return () => {
        cancelled = true;
      };
    }

    setLoading(true);
    setError(false);
    steamNewsResourceCache.read(cacheKey, () => fetchSteamNewsForApp(storeAppId, STEAM_NEWS_FETCH_COUNT, locale))
      .then((newsItems) => {
        if (cancelled) {
          return;
        }
        setItems(newsItems);
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
  }, [attempt, cacheKey, locale, storeAppId]);

  if (storeEntry?.storeSource === "official") {
    if (!officialLinks.length) {
      return null;
    }
    return (
      <section className="panel-card library-updates-strip library-updates-strip--official-source">
        <div className="library-updates-strip-head">
          <div className="eyebrow">{t("library.detail.officialUpdatesEyebrow")}</div>
          <h3>{t("library.detail.officialUpdatesTitle")}</h3>
          <p className="body-copy">{t("library.detail.officialUpdatesBody")}</p>
        </div>
        <div className="library-updates-strip-content">
          <div className="library-updates-rows">
            {officialLinks.map((link) => (
              <article key={link.id} className="library-update-row">
                <div className="library-update-side">
                  <span>{link.kind}</span>
                </div>
                <div className="library-update-main">
                  <h4 className="library-update-title">{link.label}</h4>
                  <p className="library-update-excerpt">{link.description}</p>
                  <a
                    className="library-update-link"
                    href={link.url}
                    target="_blank"
                    rel="noreferrer"
                    onClick={(event) => {
                      event.preventDefault();
                      void openExternalUrl(link.url);
                    }}
                  >
                    {t("library.detail.updatesOpen")}
                  </a>
                </div>
              </article>
            ))}
          </div>
        </div>
      </section>
    );
  }

  if (!storeAppId) {
    return null;
  }
  return (
    <section className="panel-card library-updates-strip" aria-busy={loading}>
      <div className="library-updates-strip-head">
        <div className="eyebrow">{t("library.detail.updatesEyebrow")}</div>
        <h3>{t("library.detail.updatesTitle")}</h3>
      </div>
      <div className="library-updates-strip-content">
        {loading ? <div className="form-note" role="status">{t("library.detail.updatesLoading")}</div> : null}
        {!loading && error ? <ActivityNotice tone="error" action={<button type="button" className="secondary-button" onClick={() => {
          steamNewsResourceCache.clear(cacheKey);
          setLoading(true);
          setError(false);
          setAttempt((value) => value + 1);
        }}>{t("common.retry")}</button>}>{t("library.detail.updatesError")}</ActivityNotice> : null}
        {!loading && !error && displayItems.length === 0 ? <div className="form-note">{t("library.detail.updatesEmpty")}</div> : null}
        {!loading && !error && displayItems.length ? (
          <div className="library-updates-rows">
            {displayItems.map((item) => (
              <article key={item.key} className="library-update-row">
                <div className="library-update-side">
                  <span>{item.dateLabel}</span>
                  <span>{item.sourceLabel}</span>
                </div>
                <div className="library-update-main">
                  <h4 className="library-update-title">{item.title}</h4>
                  <p className="library-update-excerpt">{item.excerpt}</p>
                  <a
                    className="library-update-link"
                    href={item.url}
                    target="_blank"
                    rel="noreferrer"
                    onClick={(event) => {
                      event.preventDefault();
                      void openExternalUrl(item.url);
                    }}
                  >
                    {t("library.detail.updatesOpen")}
                  </a>
                </div>
              </article>
            ))}
          </div>
        ) : null}
      </div>
    </section>
  );
}
