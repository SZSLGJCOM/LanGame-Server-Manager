import { isChineseLocale, type LocaleCode } from "../../i18n-config";
import type { ModuleStoreEntry } from "../../store-media";

function hasReadableChineseText(value: string) {
  return (value.match(/[\u3400-\u9fff]/gu) ?? []).length >= 4;
}

export function shouldUseLocalLibraryStory(locale: LocaleCode, storeEntry: ModuleStoreEntry | null | undefined) {
  if (!isChineseLocale(locale)) {
    return false;
  }

  if (storeEntry?.storeSource === "steam" && storeEntry.storeAppId) {
    return false;
  }

  return (storeEntry?.aboutParagraphs ?? []).some(hasReadableChineseText);
}
