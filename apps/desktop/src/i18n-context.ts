import { createContext } from "react";
import type { LocaleCode } from "./i18n-config";

export type TranslationParamValue = string | number | boolean | null | undefined;
export type TranslationParams = Record<string, TranslationParamValue>;
export type TranslateFn = (key: string, params?: TranslationParams, fallback?: string) => string;

export interface I18nContextValue {
  locale: LocaleCode;
  setLocale: (locale: LocaleCode) => void;
  t: TranslateFn;
}

// Keep context identity independent of catalog and provider hot updates.
export const I18nContext = createContext<I18nContextValue | null>(null);
