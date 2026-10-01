import type { TranslateFn, TranslationParams } from "./i18n";
import type { ProgramCleanupResult } from "./types";

import { formatDesktopError } from "./desktop-error-message";

export interface UiMessage {
  key: string;
  params?: TranslationParams;
  fallback?: string;
  scopeKey?: string;
  tone?: "error" | "warning";
}

export function message(key: string, params?: TranslationParams, extra?: Omit<UiMessage, "key" | "params">): UiMessage {
  return { key, params, ...extra };
}

export function resolveUiMessage(t: TranslateFn, value: UiMessage): string {
  const params: TranslationParams = { ...(value.params ?? {}) };
  if (typeof params.message === "string") {
    params.message = formatDesktopError(t, params.message);
  }
  if (value.scopeKey) {
    params.scopeKey = t(`scopes.${value.scopeKey}`, undefined, value.scopeKey);
  }
  return t(value.key, params, value.fallback);
}

export function programCleanupDetails(cleanup: ProgramCleanupResult, t: TranslateFn): string {
  return [
    ...(cleanup.preserved_data_paths.length ? [t("programCleanup.preservedData", {
      paths: [...new Set(cleanup.preserved_data_paths)].join("\n")
    })] : []),
    ...(cleanup.retained_installs.length ? [t("programCleanup.retainedPrograms", {
      paths: cleanup.retained_installs.map((entry) => `${entry.install_root}\n${t(
        `programCleanup.reason.${entry.reason}`, undefined, entry.reason
      )}`).join("\n\n")
    })] : [])
  ].join("\n\n");
}
