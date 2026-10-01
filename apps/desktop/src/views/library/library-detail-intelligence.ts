import type { ModuleOfficialLink, ModuleStoreEntry } from "../../store-media";
import { getModulePresentationFallbackMedia } from "../../module-presentation";
import type { SteamNewsItem, SteamReviewSummary } from "../../types";

export interface LibraryDetailProfile {
  moduleId: string;
  storeSource: ModuleStoreEntry["storeSource"] | "missing";
  steamAppId: number | null;
  storeName: string;
  shortDescription: string;
  aboutParagraphCount: number;
  genreCount: number;
  categoryCount: number;
  releaseDate: string;
  developer: string;
  publisher: string;
  storeUrl: string;
  officialLinks: ModuleOfficialLink[];
  screenshotCount: number;
  trailerCount: number;
  mediaCount: number;
  canFetchSteamNews: boolean;
  canFetchSteamReviewSummary: boolean;
  hasIntroduction: boolean;
  hasMedia: boolean;
  reviewSignalSource: "steam" | "none";
  updateSource: "steam-news" | "official-links" | "none";
  missing: string[];
  detailContentIssues: string[];
}

export interface FormattedSteamReviewSummary {
  scoreLabel: string;
  positivePercentLabel: string;
  totalReviewsLabel: string;
  sourceUrl: string;
}

export interface FormattedSteamNewsDigestItem {
  key: string;
  title: string;
  url: string;
  sourceLabel: string;
  dateLabel: string;
  excerpt: string;
  publishedAtUnixMs: number;
}

export interface LibraryDetailResourceCacheOptions<T = unknown> {
  ttlMs: number;
  now?: () => number;
  shouldCache?: (value: T) => boolean;
  maxEntries?: number;
}

function hasValue(value: string | null | undefined) {
  return Boolean(value && value.trim());
}

function cleanSteamNewsExcerpt(excerpt: string) {
  return excerpt
    .replace(/<[^>]+>/g, " ")
    .replace(/&nbsp;/gi, " ")
    .replace(/&amp;/gi, "&")
    .replace(/&quot;/gi, "\"")
    .replace(/&#39;|&apos;/gi, "'")
    .replace(/&lt;/gi, "<")
    .replace(/&gt;/gi, ">")
    .replace(/\s+/g, " ")
    .replace(/\s+([,.;:!?])/g, "$1")
    .trim();
}

function countTotalReviews(summary: SteamReviewSummary) {
  const explicitTotal = Number(summary.total_reviews);
  if (Number.isFinite(explicitTotal) && explicitTotal > 0) {
    return explicitTotal;
  }

  const positive = Number(summary.total_positive);
  const negative = Number(summary.total_negative);
  return Math.max(0, positive) + Math.max(0, negative);
}

function resolvePositivePercent(summary: SteamReviewSummary) {
  const explicitPercent = Number(summary.positive_percent);
  if (Number.isFinite(explicitPercent) && explicitPercent >= 0) {
    return Math.round(Math.min(explicitPercent, 100));
  }

  const positive = Math.max(0, Number(summary.total_positive));
  const total = countTotalReviews(summary);
  if (total <= 0) {
    return 0;
  }

  return Math.round((positive / total) * 100);
}

function buildDetailContentIssues(profile: {
  hasIntroduction: boolean;
  hasMedia: boolean;
  reviewSignalSource: LibraryDetailProfile["reviewSignalSource"];
  updateSource: LibraryDetailProfile["updateSource"];
  storeSource: LibraryDetailProfile["storeSource"];
}) {
  const issues: string[] = [];
  if (!profile.hasIntroduction) {
    issues.push("missing introduction");
  }
  if (!profile.hasMedia && profile.storeSource === "steam") {
    issues.push("missing media");
  }
  if (profile.storeSource === "steam" && profile.reviewSignalSource === "none") {
    issues.push("missing Steam review source");
  }
  if (profile.updateSource === "none") {
    issues.push("missing update source");
  }
  return issues;
}

export function buildLibraryDetailProfile(moduleId: string, storeEntry: ModuleStoreEntry | null | undefined): LibraryDetailProfile {
  const missing: string[] = [];
  if (!storeEntry) {
    return {
      moduleId,
      storeSource: "missing",
      steamAppId: null,
      storeName: "",
      shortDescription: "",
      aboutParagraphCount: 0,
      genreCount: 0,
      categoryCount: 0,
      releaseDate: "",
      developer: "",
      publisher: "",
      storeUrl: "",
      officialLinks: [],
      screenshotCount: 0,
      trailerCount: 0,
      mediaCount: 0,
      canFetchSteamNews: false,
      canFetchSteamReviewSummary: false,
      hasIntroduction: false,
      hasMedia: false,
      reviewSignalSource: "none",
      updateSource: "none",
      missing: ["storeEntry"],
      detailContentIssues: ["missing introduction", "missing media", "missing update source"]
    };
  }

  const steamAppId = storeEntry.storeSource === "steam" && storeEntry.storeAppId ? storeEntry.storeAppId : null;
  const screenshotCount = storeEntry.screenshots.length;
  const trailerCount = storeEntry.trailers.length;
  const storeMediaCount = screenshotCount + trailerCount;
  const presentationMediaCount = storeMediaCount === 0
    ? getModulePresentationFallbackMedia(moduleId).length
    : 0;
  const mediaCount = storeMediaCount + presentationMediaCount;
  const hasIntroduction = hasValue(storeEntry.shortDescription) || storeEntry.aboutParagraphs.some(hasValue);
  const hasMedia = mediaCount > 0;
  const reviewSignalSource: LibraryDetailProfile["reviewSignalSource"] = steamAppId !== null ? "steam" : "none";
  const updateSource: LibraryDetailProfile["updateSource"] = steamAppId !== null
    ? "steam-news"
    : storeEntry.officialLinks.length > 0
      ? "official-links"
      : "none";

  if (!hasValue(storeEntry.storeName)) missing.push("storeName");
  if (!hasValue(storeEntry.shortDescription)) missing.push("shortDescription");
  if (!storeEntry.aboutParagraphs.length) missing.push("aboutParagraphs");
  if (!storeEntry.genres.length) missing.push("genres");
  if (!storeEntry.categories.length) missing.push("categories");
  if (!hasValue(storeEntry.releaseDate)) missing.push("releaseDate");
  if (!storeEntry.developers.length) missing.push("developers");
  if (!storeEntry.publishers.length) missing.push("publishers");
  if (!hasValue(storeEntry.storeUrl)) missing.push("storeUrl");

  return {
    moduleId,
    storeSource: storeEntry.storeSource,
    steamAppId,
    storeName: storeEntry.storeName,
    shortDescription: storeEntry.shortDescription,
    aboutParagraphCount: storeEntry.aboutParagraphs.length,
    genreCount: storeEntry.genres.length,
    categoryCount: storeEntry.categories.length,
    releaseDate: storeEntry.releaseDate,
    developer: storeEntry.developers[0] ?? "",
    publisher: storeEntry.publishers[0] ?? "",
    storeUrl: storeEntry.storeUrl,
    officialLinks: storeEntry.officialLinks,
    screenshotCount,
    trailerCount,
    mediaCount,
    canFetchSteamNews: steamAppId !== null,
    canFetchSteamReviewSummary: steamAppId !== null,
    hasIntroduction,
    hasMedia,
    reviewSignalSource,
    updateSource,
    missing,
    detailContentIssues: buildDetailContentIssues({
      hasIntroduction,
      hasMedia,
      reviewSignalSource,
      updateSource,
      storeSource: storeEntry.storeSource
    })
  };
}

export function shouldFetchSteamReviewSummary(profile: Pick<LibraryDetailProfile, "canFetchSteamReviewSummary" | "steamAppId">) {
  return profile.canFetchSteamReviewSummary && profile.steamAppId !== null;
}

export function formatSteamReviewSummary(summary: SteamReviewSummary, locale: string): FormattedSteamReviewSummary {
  const totalReviews = countTotalReviews(summary);
  const scoreLabel = hasValue(summary.review_score_desc) ? summary.review_score_desc.trim() : "Reviews";

  return {
    scoreLabel,
    positivePercentLabel: `${resolvePositivePercent(summary)}%`,
    totalReviewsLabel: new Intl.NumberFormat(locale).format(totalReviews),
    sourceUrl: summary.source_url
  };
}

export function formatSteamNewsDigest(items: SteamNewsItem[], locale: string, limit = 3): FormattedSteamNewsDigestItem[] {
  const seen = new Set<string>();
  const deduped: SteamNewsItem[] = [];
  for (const item of items) {
    const title = item.title.trim();
    const url = item.url.trim();
    if (!title || !url) {
      continue;
    }

    const dedupeKey = url || item.gid || title;
    if (seen.has(dedupeKey)) {
      continue;
    }
    seen.add(dedupeKey);
    deduped.push(item);
  }

  return deduped
    .sort((left, right) => right.published_at_unix_ms - left.published_at_unix_ms)
    .slice(0, Math.max(0, limit))
    .map((item) => {
      const sourceLabel = hasValue(item.feed_label)
        ? item.feed_label!.trim()
        : hasValue(item.author)
          ? item.author!.trim()
          : "Steam News";

      return {
        key: item.gid || item.url,
        title: item.title.trim(),
        url: item.url.trim(),
        sourceLabel,
        dateLabel: new Intl.DateTimeFormat(locale, {
          year: "numeric",
          month: "short",
          day: "numeric"
        }).format(item.published_at_unix_ms),
        excerpt: cleanSteamNewsExcerpt(item.excerpt),
        publishedAtUnixMs: item.published_at_unix_ms
      };
    });
}

export function createLibraryDetailResourceCache<T>(options: LibraryDetailResourceCacheOptions<T>) {
  const entries = new Map<string, { expiresAt: number; promise: Promise<T> }>();
  const now = options.now ?? (() => Date.now());
  const ttlMs = Math.max(0, options.ttlMs);

  return {
    read(key: string, loader: () => Promise<T>) {
      const currentTime = now();
      const cached = entries.get(key);
      if (cached && currentTime < cached.expiresAt) {
        return cached.promise;
      }

      const promise = Promise.resolve().then(loader).then((value) => {
        if (options.shouldCache && !options.shouldCache(value) && entries.get(key)?.promise === promise) {
          entries.delete(key);
        }
        return value;
      });
      entries.delete(key);
      entries.set(key, { expiresAt: currentTime + ttlMs, promise });
      while (entries.size > Math.max(1, options.maxEntries ?? Infinity)) {
        const oldest = entries.keys().next().value;
        if (oldest !== undefined) entries.delete(oldest);
      }
      promise.catch(() => {
        if (entries.get(key)?.promise === promise) {
          entries.delete(key);
        }
      });
      return promise;
    },
    clear(key?: string) {
      if (key) {
        entries.delete(key);
        return;
      }
      entries.clear();
    }
  };
}
