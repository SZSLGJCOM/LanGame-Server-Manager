import { invoke, isTauri } from "@tauri-apps/api/core";
import type { LocaleCode } from "./i18n-config";

// Keep one native update in flight and only the newest pending preference.
// Separate IPC requests may complete out of order, including transport fallback.
export function createTrayLocaleSync(apply: (locale: LocaleCode) => Promise<void>) {
  let pending: LocaleCode | null = null;
  let running = false;

  async function drain() {
    while (pending !== null) {
      const locale = pending;
      pending = null;
      try {
        await apply(locale);
      } catch (error) {
        console.error("Unable to update tray language", error);
      }
    }
    running = false;
  }

  return (locale: LocaleCode) => {
    pending = locale;
    if (!running) {
      running = true;
      void drain();
    }
  };
}

const sync = createTrayLocaleSync((locale) => invoke<void>("set_tray_locale", { locale }));

export function syncDesktopTrayLocale(locale: LocaleCode): void {
  if (isTauri()) sync(locale);
}
