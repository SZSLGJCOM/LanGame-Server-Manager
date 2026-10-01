import rawStoreData from "./data/module-store-data.json";
import { isChineseLocale, type LocaleCode, type TranslateFn } from "./i18n";
import { getModulePresentationFallbackMedia } from "./module-presentation";
import { isOfficialMediaUrl } from "./official-media-sources";
import { EN_US_MODULE_STORE_COPY, ZH_CN_MODULE_STORE_COPY, type ModuleStoreCopy } from "./store-copy";

interface RawModuleScreenshot {
  id: number;
  label: string;
  sourceUrl: string;
}

interface RawModuleTrailer {
  id: number;
  name: string;
  posterUrl: string | null;
  streamUrl: string | null;
  highlight: boolean;
}

interface RawModuleOfficialLink {
  id: string;
  label: string;
  description: string;
  url: string;
  kind: string;
}

interface RawModuleStoreEntry {
  storeSource?: "steam" | "official";
  storeAppId: number | null;
  storeName: string;
  coverUrl: string | null;
  shortDescription: string;
  aboutParagraphs: string[];
  genres: string[];
  categories: string[];
  developers: string[];
  publishers: string[];
  releaseDate: string;
  storeUrl: string;
  officialLinks?: RawModuleOfficialLink[];
  screenshots: RawModuleScreenshot[];
  trailers: RawModuleTrailer[];
}

export interface ModuleScreenshot {
  id: number;
  label: string;
  sourceUrl: string;
  src: string;
}

export interface ModuleTrailer {
  id: number;
  name: string;
  posterUrl: string | null;
  posterSrc: string | null;
  streamUrl: string | null;
  highlight: boolean;
}

export interface ModuleOfficialLink {
  id: string;
  label: string;
  description: string;
  url: string;
  kind: string;
}

export interface ModuleStoreEntry {
  storeSource: "steam" | "official";
  storeAppId: number | null;
  storeName: string;
  coverUrl: string | null;
  shortDescription: string;
  aboutParagraphs: string[];
  genres: string[];
  categories: string[];
  developers: string[];
  publishers: string[];
  releaseDate: string;
  storeUrl: string;
  officialLinks: ModuleOfficialLink[];
  screenshots: ModuleScreenshot[];
  trailers: ModuleTrailer[];
}

export interface ModuleMediaItem {
  key: string;
  kind: "trailer" | "screenshot" | "cover";
  title: string;
  badge: string;
  thumbnailSrc: string | null;
  imageSrc: string | null;
  streamUrl: string | null;
}

const ALLOWED_STORE_MEDIA_HOSTS = new Set([
  "cdn.akamai.steamstatic.com",
  "shared.akamai.steamstatic.com",
  "shared.fastly.steamstatic.com",
  "video.akamai.steamstatic.com",
  "store-images.s-microsoft.com",
  "cdn.trailers.xboxservices.com"
]);

export function resolveStoreMediaUrl(value: string | null | undefined): string | null {
  const candidate = value?.trim();
  if (!candidate) {
    return null;
  }

  try {
    const url = new URL(candidate);
    if (
      url.protocol !== "https:"
      || !(ALLOWED_STORE_MEDIA_HOSTS.has(url.hostname) ||
        isOfficialMediaUrl(candidate, "image") || isOfficialMediaUrl(candidate, "video"))
      || Boolean(url.username)
      || Boolean(url.password)
      || Boolean(url.port)
    ) {
      return null;
    }
    return url.toString();
  } catch {
    return null;
  }
}

const utf8Decoder = new TextDecoder();

function scoreImportedText(value: string) {
  const chineseCount = (value.match(/[\u3400-\u9fff]/g) ?? []).length;
  const latinCount = (value.match(/[À-ÿ]/g) ?? []).length;
  const replacementCount = (value.match(/[�]/g) ?? []).length;
  return chineseCount * 2 - latinCount * 2 - replacementCount * 4;
}

function repairLatin1Mojibake(value: string) {
  const latinMatches = value.match(/[À-ÿ]/g) ?? [];
  if (latinMatches.length < 4) {
    return value;
  }

  try {
    const bytes = Uint8Array.from(value, (char) => {
      const code = char.charCodeAt(0);
      return code <= 0xff ? code : 0x3f;
    });
    const repaired = utf8Decoder.decode(bytes);
    return scoreImportedText(repaired) > scoreImportedText(value) ? repaired : value;
  } catch {
    return value;
  }
}

function normalizeText(value: string) {
  return repairLatin1Mojibake(value).trim();
}

function countChineseCharacters(value: string) {
  return (value.match(/[\u3400-\u9fff]/g) ?? []).length;
}

function countLatinCharacters(value: string) {
  return (value.match(/[a-z]/gi) ?? []).length;
}

function isUsefulStoreParagraph(value: string, locale: LocaleCode) {
  const normalized = normalizeText(value).replace(/\s+/g, " ").trim();
  if (normalized.length < 28) {
    return false;
  }
  if (/官方\s*QQ|QQ群|加群|discord|^q[.:：]/i.test(normalized)) {
    return false;
  }

  if (isChineseLocale(locale)) {
    return countChineseCharacters(normalized) >= 10;
  }

  return countLatinCharacters(normalized) >= 24;
}

function appendUniqueParagraph(target: string[], value: string) {
  const normalized = normalizeText(value)
    .replace(/\s+/g, " ")
    .replace(/^[QA][.:：]\s*/i, "")
    .trim();
  if (!normalized) {
    return;
  }

  const normalizedKey = normalized.toLowerCase();
  if (target.some((paragraph) => paragraph.toLowerCase() === normalizedKey)) {
    return;
  }
  target.push(normalized);
}

function buildStoreMetadataParagraph(entry: ModuleStoreEntry, copy: ModuleStoreCopy, locale: LocaleCode) {
  const releaseDate = copy.releaseDate || entry.releaseDate;
  const developer = entry.developers[0] ?? "";
  const publisher = entry.publishers[0] ?? "";
  const isDisplayTag = (value: string) => !/dedicated server|community servers?|专用服务器|社区服务器/i.test(value);
  const genres = copy.genres.filter(isDisplayTag).slice(0, 4).join(isChineseLocale(locale) ? "、" : ", ");
  const categories = copy.categories.filter(isDisplayTag).slice(0, 5).join(isChineseLocale(locale) ? "、" : ", ");

  if (isChineseLocale(locale)) {
    return [
      genres ? `${copy.storeName} 属于${genres}。` : `${copy.storeName} 是当前资料库中的游戏条目。`,
      categories ? `常见玩法标签包括${categories}。` : "",
      releaseDate ? `发行日期：${releaseDate}。` : "",
      developer ? `开发商：${developer}。` : "",
      publisher ? `发行商：${publisher}。` : ""
    ].filter(Boolean).join("");
  }

  return [
    genres ? `${copy.storeName} is listed as ${genres}.` : `${copy.storeName} is part of the game library.`,
    categories ? ` Common tags include ${categories}.` : "",
    releaseDate ? ` Release date: ${releaseDate}.` : "",
    developer ? ` Developer: ${developer}.` : "",
    publisher ? ` Publisher: ${publisher}.` : ""
  ].join("");
}

function buildLocalizedAboutParagraphs(entry: ModuleStoreEntry, copy: ModuleStoreCopy, locale: LocaleCode) {
  const paragraphs: string[] = [];

  const localizedShortDescription = isUsefulStoreParagraph(copy.shortDescription, locale)
    ? copy.shortDescription
    : entry.shortDescription;
  appendUniqueParagraph(paragraphs, localizedShortDescription || entry.shortDescription);
  for (const paragraph of (copy.storyParagraphs ?? []).filter((item) => isUsefulStoreParagraph(item, locale))) {
    appendUniqueParagraph(paragraphs, paragraph);
  }
  for (const paragraph of entry.aboutParagraphs.filter((item) => isUsefulStoreParagraph(item, locale)).slice(0, 6)) {
    appendUniqueParagraph(paragraphs, paragraph);
  }

  const characterCount = paragraphs.join("").replace(/\s/g, "").length;
  if (paragraphs.length < 4 || characterCount < 320) {
    appendUniqueParagraph(paragraphs, buildStoreMetadataParagraph(entry, copy, locale));
  }

  return paragraphs;
}

function shouldUseFallbackLabel(value: string | null | undefined, locale: LocaleCode): boolean {
  if (!value) {
    return true;
  }

  const normalized = value.trim();
  if (!normalized) {
    return true;
  }

  if (/^\?+\s*\d*$/.test(normalized)) {
    return true;
  }

  if (locale === "en-US" && /[\u3400-\u9fff]/.test(normalized)) {
    return true;
  }

  if (isChineseLocale(locale) && /^(?:screenshot|trailer)(?:\s+\d+)?$/i.test(normalized)) {
    return true;
  }

  if (isChineseLocale(locale) && /\btrailer\b/i.test(normalized) && !/[\u3400-\u9fff]/.test(normalized)) {
    return true;
  }

  return false;
}

const storeDataByModule = Object.fromEntries(
  Object.entries(rawStoreData as Record<string, RawModuleStoreEntry>).map(([moduleId, entry]) => [
    moduleId,
    {
      ...entry,
      storeSource: entry.storeSource ?? (entry.storeAppId === null ? "official" : "steam"),
      storeName: normalizeText(entry.storeName),
      coverUrl: resolveStoreMediaUrl(entry.coverUrl),
      shortDescription: normalizeText(entry.shortDescription),
      aboutParagraphs: entry.aboutParagraphs.map(normalizeText),
      genres: entry.genres.map(normalizeText),
      categories: entry.categories.map(normalizeText),
      developers: entry.developers.map(normalizeText),
      publishers: entry.publishers.map(normalizeText),
      releaseDate: normalizeText(entry.releaseDate),
      officialLinks: (entry.officialLinks ?? []).map((link) => ({
        ...link,
        id: normalizeText(link.id),
        label: normalizeText(link.label),
        description: normalizeText(link.description),
        kind: normalizeText(link.kind),
        url: link.url.trim()
      })),
      screenshots: entry.screenshots.flatMap((screenshot) => {
        const src = resolveStoreMediaUrl(screenshot.sourceUrl);
        return src ? [{ ...screenshot, sourceUrl: src, label: normalizeText(screenshot.label), src }] : [];
      }),
      trailers: entry.trailers.flatMap((trailer) => {
        const streamUrl = resolveStoreMediaUrl(trailer.streamUrl);
        if (!streamUrl) {
          return [];
        }
        const posterUrl = resolveStoreMediaUrl(trailer.posterUrl);
        return [{
          ...trailer,
          name: normalizeText(trailer.name),
          posterUrl,
          posterSrc: posterUrl,
          streamUrl
        }];
      })
    }
  ])
) as Record<string, ModuleStoreEntry>;

export function getModuleStoreData(moduleId: string | null | undefined): ModuleStoreEntry | null {
  if (!moduleId) {
    return null;
  }
  return storeDataByModule[moduleId] ?? null;
}

export function getLocalizedModuleDisplayName(
  moduleId: string | null | undefined,
  locale: LocaleCode,
  fallback?: string | null
): string {
  const copy = isChineseLocale(locale)
    ? ZH_CN_MODULE_STORE_COPY[moduleId ?? ""]
    : EN_US_MODULE_STORE_COPY[moduleId ?? ""];

  return copy?.storeName ?? getModuleStoreData(moduleId)?.storeName ?? fallback ?? moduleId ?? "";
}

export function getLocalizedModuleStoreData(
  moduleId: string | null | undefined,
  locale: LocaleCode
): ModuleStoreEntry | null {
  const entry = getModuleStoreData(moduleId);
  if (!entry) {
    return entry;
  }

  const copy = isChineseLocale(locale)
    ? ZH_CN_MODULE_STORE_COPY[moduleId ?? ""]
    : EN_US_MODULE_STORE_COPY[moduleId ?? ""];
  if (!copy) {
    return entry;
  }

  return {
    ...entry,
    storeName: copy.storeName,
    shortDescription: copy.shortDescription,
    aboutParagraphs: buildLocalizedAboutParagraphs(entry, copy, locale),
    genres: copy.genres,
    categories: copy.categories,
    releaseDate: copy.releaseDate,
    officialLinks: copy.officialLinks?.map((link) => ({
      ...link,
      id: normalizeText(link.id),
      label: normalizeText(link.label),
      description: normalizeText(link.description),
      kind: normalizeText(link.kind),
      url: link.url.trim()
    })) ?? entry.officialLinks
  };
}

export function buildModuleMediaItems(
  moduleId: string | null | undefined,
  storeEntry: ModuleStoreEntry | null,
  locale: LocaleCode,
  t: TranslateFn
): ModuleMediaItem[] {
  const storeMediaItems: ModuleMediaItem[] = [];
  if (storeEntry) {
    const fallbackThumb = storeEntry.screenshots[0]?.src ?? null;
    storeMediaItems.push(
      ...storeEntry.trailers.map((trailer, index) => {
        const fallbackLabel = t("library.media.trailerBadge", { index: index + 1 });
        const title = shouldUseFallbackLabel(trailer.name, locale) ? fallbackLabel : trailer.name;

        return {
          key: `trailer-${trailer.id}`,
          kind: "trailer" as const,
          title,
          badge: fallbackLabel,
          thumbnailSrc: trailer.posterSrc ?? fallbackThumb,
          imageSrc: trailer.posterSrc ?? fallbackThumb,
          streamUrl: trailer.streamUrl
        };
      }),
      ...storeEntry.screenshots.map((screenshot, index) => {
        const fallbackLabel = t("library.media.screenshotBadge", { index: index + 1 });
        const title = shouldUseFallbackLabel(screenshot.label, locale) ? fallbackLabel : screenshot.label;

        return {
          key: `screenshot-${screenshot.id}`,
          kind: "screenshot" as const,
          title,
          badge: fallbackLabel,
          thumbnailSrc: screenshot.src,
          imageSrc: screenshot.src,
          streamUrl: null
        };
      })
    );
  }

  if (storeMediaItems.length > 0) {
    return storeMediaItems;
  }

  const presentationFallback = getModulePresentationFallbackMedia(moduleId ?? "");
  return presentationFallback.map((media) => ({
    key: `presentation-${media.id}`,
    kind: "cover" as const,
    title: t(media.titleKey),
    badge: t(media.badgeKey),
    thumbnailSrc: media.imageSrc,
    imageSrc: media.imageSrc,
    streamUrl: null
  }));
}
