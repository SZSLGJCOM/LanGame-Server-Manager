import { FALLBACK_LOCALE, isChineseLocale, SUPPORTED_LOCALES, type LocaleCode } from "./i18n-config";

const STORAGE_KEY = "langame.locale";

export function normalizeLocale(locale?: string | null): LocaleCode {
  if (!locale) return FALLBACK_LOCALE;
  const direct = SUPPORTED_LOCALES.find((candidate) => candidate.toLowerCase() === locale.toLowerCase());
  return direct ?? (isChineseLocale(locale) ? "zh-CN" : "en-US");
}

// React and request dispatch read the same preference, including before the first render.
export function readPreferredLocale(): LocaleCode {
  if (typeof window === "undefined") return FALLBACK_LOCALE;
  return normalizeLocale(window.localStorage.getItem(STORAGE_KEY) || window.navigator.language);
}

export function writePreferredLocale(locale: LocaleCode): void {
  if (typeof window !== "undefined") window.localStorage.setItem(STORAGE_KEY, locale);
}
