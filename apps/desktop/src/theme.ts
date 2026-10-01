import type { ThemeMode } from "./types";

const STORAGE_KEY = "langame.theme";

export function normalizeTheme(theme?: string | null): ThemeMode {
  return theme === "dark" ? "dark" : "light";
}

export function detectInitialTheme(): ThemeMode {
  if (typeof window === "undefined") {
    return "light";
  }

  const stored = window.localStorage.getItem(STORAGE_KEY);
  if (stored) {
    return normalizeTheme(stored);
  }

  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function applyThemeToDocument(theme: ThemeMode) {
  if (typeof document === "undefined") {
    return;
  }

  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
}

export function persistTheme(theme: ThemeMode) {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(STORAGE_KEY, theme);
}

export const INITIAL_THEME = detectInitialTheme();

if (typeof document !== "undefined" && !document.documentElement.dataset.theme) {
  applyThemeToDocument(INITIAL_THEME);
}
