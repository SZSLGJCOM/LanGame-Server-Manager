export const SUPPORTED_LOCALES = ["zh-CN", "en-US"] as const;

export type LocaleCode = (typeof SUPPORTED_LOCALES)[number];

export const FALLBACK_LOCALE: LocaleCode = "zh-CN";

export type MessageCatalog = Record<string, string>;

export function isChineseLocale(locale: string | null | undefined): boolean {
  const language = String(locale ?? "").trim().toLowerCase();
  return language.slice(0, 2) === "zh";
}

export function selectLocaleText(locale: string | null | undefined, zhCn: string, enUs: string): string {
  return isChineseLocale(locale) ? zhCn : enUs;
}
