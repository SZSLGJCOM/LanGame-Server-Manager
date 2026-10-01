import { useCallback, useContext, useEffect, useMemo, useState } from "react";
import {
  isChineseLocale,
  selectLocaleText,
  SUPPORTED_LOCALES,
  type LocaleCode,
  type MessageCatalog
} from "./i18n-config";
import { createCatalogLoader } from "./i18n-catalog-loader";
import { I18nContext, type I18nContextValue, type TranslationParams } from "./i18n-context";
import { readPreferredLocale, writePreferredLocale } from "./locale-preference";

export type { LocaleCode } from "./i18n-config";
export type { TranslateFn, TranslationParams, TranslationParamValue } from "./i18n-context";
export { isChineseLocale, selectLocaleText } from "./i18n-config";
export { normalizeLocale } from "./locale-preference";

type MessageValue = string | { [key: string]: MessageValue };
const catalogCache: Partial<Record<LocaleCode, MessageCatalog>> = {};

async function importLocaleCatalog(locale: LocaleCode): Promise<MessageCatalog> {
  switch (locale) {
    case "en-US":
      return (await import("./i18n-messages")).EN_US_MESSAGES;
    case "zh-CN":
      return (await import("./i18n-messages-zh-cn")).ZH_CN_MESSAGES;
    default:
      return (await import("./i18n-messages-zh-cn")).ZH_CN_MESSAGES;
  }
}

const localeCatalogLoader = createCatalogLoader(catalogCache, importLocaleCatalog);

function storeCatalog(
  current: Partial<Record<LocaleCode, MessageCatalog>>,
  locale: LocaleCode,
  catalog: MessageCatalog
) {
  if (current[locale] === catalog) {
    return current;
  }

  return {
    ...current,
    [locale]: catalog
  };
}

function I18nLoadingState({ locale }: { locale: LocaleCode }) {
  const loadingLabel = selectLocaleText(locale, "\u6b63\u5728\u52a0\u8f7d\u754c\u9762...", "Loading interface...");

  return (
    <div
      aria-live="polite"
      role="status"
      style={{
        minHeight: "100vh",
        display: "grid",
        placeItems: "center",
        padding: "24px",
        background: "var(--bg)",
        color: "var(--text)",
        fontSize: "14px",
        letterSpacing: "0.02em"
      }}
    >
      {loadingLabel}
    </div>
  );
}

function I18nErrorState({
  locale,
  error,
  onRetry
}: {
  locale: LocaleCode;
  error: string;
  onRetry: () => void;
}) {
  const message = selectLocaleText(
    locale,
    "\u7ffb\u8bd1\u6587\u4ef6\u52a0\u8f7d\u5931\u8d25\uff0c\u8bf7\u5c1d\u8bd5\u5237\u65b0\u6216\u91cd\u65b0\u542f\u52a8\u3002",
    "Locale catalog failed to load. Please retry or restart."
  );

  return (
    <div
      aria-live="assertive"
      role="status"
      style={{
        minHeight: "100vh",
        display: "grid",
        placeItems: "center",
        padding: "24px",
        background: "var(--bg)",
        color: "var(--text)",
        fontSize: "14px",
        letterSpacing: "0.02em"
      }}
    >
      <div style={{ maxWidth: "560px", display: "grid", gap: "12px" }}>
        <p style={{ margin: 0 }}>{message}</p>
        <pre
          style={{
            margin: 0,
            padding: "12px",
            borderRadius: "10px",
            border: "1px solid var(--theme-danger-border)",
            background: "var(--theme-danger-bg)",
            color: "var(--theme-danger-text)",
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            maxHeight: "160px",
            overflow: "auto"
          }}
        >
          {error}
        </pre>
        <div style={{ display: "flex", gap: "12px", flexWrap: "wrap" }}>
          <button
            type="button"
            onClick={onRetry}
            style={{
              padding: "8px 12px",
              borderRadius: "8px",
              border: "1px solid var(--border)",
              background: "var(--theme-control-surface)",
              color: "var(--text)",
              cursor: "pointer"
            }}
          >
            {selectLocaleText(locale, "\u5237\u65b0", "Retry")}
          </button>
        </div>
      </div>
    </div>
  );
}

function describeCatalogError(error: unknown): string {
  return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}

function lookupMessage(tree: MessageValue | undefined, key: string): string | undefined {
  if (!tree || typeof tree === "string") {
    return undefined;
  }

  const directMatch = tree[key];
  if (typeof directMatch === "string") {
    return directMatch;
  }

  return String(key)
    .split(".")
    .reduce<MessageValue | undefined>((current, part) => {
      if (!current || typeof current === "string") {
        return undefined;
      }
      return current[part];
    }, tree) as string | undefined;
}

function formatMessage(template: string, params: TranslationParams = {}): string {
  return template.replace(/\{\s*([\w.]+)\s*\}/g, (match, key) => {
    const value = params[key];
    return value == null ? match : String(value);
  });
}

export function translate(
  locale: LocaleCode,
  key: string,
  params?: TranslationParams,
  fallback = key,
  catalogs: Partial<Record<LocaleCode, MessageCatalog>> = catalogCache
): string {
  const activeTree = catalogs[locale] as MessageValue | undefined;
  const englishTree = catalogs["en-US"] as MessageValue | undefined;
  const chineseTree = catalogs["zh-CN"] as MessageValue | undefined;
  const activeMessage = lookupMessage(activeTree, key);
  if (activeMessage) {
    return formatMessage(activeMessage, params);
  }

  const englishMessage = locale === "en-US" ? undefined : lookupMessage(englishTree, key);
  if (englishMessage) {
    return formatMessage(englishMessage, params);
  }

  if (fallback !== key) {
    return formatMessage(fallback, params);
  }

  const chineseMessage = isChineseLocale(locale) ? undefined : lookupMessage(chineseTree, key);
  const template = chineseMessage ?? fallback;
  return formatMessage(template, params);
}

export function formatDateTime(
  locale: LocaleCode,
  value: string | number | Date | null | undefined,
  options: Intl.DateTimeFormatOptions
): string {
  if (!value) {
    return "-";
  }

  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) {
    return String(value);
  }

  return new Intl.DateTimeFormat(locale, options).format(date);
}

export function I18nProvider({ children }: { children: React.ReactNode }) {
  const [locale, setLocaleState] = useState<LocaleCode>(readPreferredLocale);
  const setLocale = useCallback((nextLocale: LocaleCode) => {
    // Persist before rendering so requests issued in this event see the new language.
    writePreferredLocale(nextLocale);
    setLocaleState(nextLocale);
  }, []);
  const [catalogs, setCatalogs] = useState<Partial<Record<LocaleCode, MessageCatalog>>>({});
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loadAttempt, setLoadAttempt] = useState(0);

  useEffect(() => {
    if (typeof document !== "undefined") {
      document.documentElement.lang = locale;
    }
  }, [locale]);

  useEffect(() => {
    let cancelled = false;

    if (catalogs[locale]) {
      if (loadError) {
        setLoadError(null);
      }
      return undefined;
    }

    void localeCatalogLoader.load(locale)
      .then((catalog) => {
        if (cancelled) {
          return;
        }
        setCatalogs((current) => storeCatalog(current, locale, catalog));
        setLoadError(null);
      })
      .catch((error) => {
        if (!cancelled) {
          setLoadError(describeCatalogError(error));
        }
      });

    return () => {
      cancelled = true;
    };
  }, [catalogs, loadAttempt, locale]);

  useEffect(() => {
    let cancelled = false;

    if (!catalogs[locale]) {
      return undefined;
    }

    for (const candidate of SUPPORTED_LOCALES) {
      if (candidate === locale || catalogs[candidate]) {
        continue;
      }

      void localeCatalogLoader.load(candidate)
        .then((catalog) => {
          if (cancelled) {
            return;
          }
          setCatalogs((current) => storeCatalog(current, candidate, catalog));
        })
        .catch((error) => {
          console.warn(`Locale catalog preload failed for ${candidate}.`, error);
        });
    }

    return () => {
      cancelled = true;
    };
  }, [catalogs, locale]);

  const t = useCallback(
    (key: string, params?: TranslationParams, fallback?: string) => translate(locale, key, params, fallback, catalogs),
    [catalogs, locale]
  );

  const contextValue: I18nContextValue = useMemo(
    () => ({
      locale,
      setLocale,
      t
    }),
    [locale, t]
  );

  if (loadError && !catalogs[locale]) {
    return (
      <I18nErrorState
        locale={locale}
        error={loadError}
        onRetry={() => {
          localeCatalogLoader.invalidate(locale);
          setLoadError(null);
          setCatalogs((current) => {
            const next = { ...current };
            delete next[locale];
            return next;
          });
          setLoadAttempt((current) => current + 1);
        }}
      />
    );
  }

  if (!catalogs[locale]) {
    return <I18nLoadingState locale={locale} />;
  }

  return <I18nContext.Provider value={contextValue}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  const value = useContext(I18nContext);
  if (!value) {
    throw new Error("useI18n must be used inside I18nProvider");
  }
  return value;
}
