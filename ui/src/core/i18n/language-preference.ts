export const SUPPORTED_LANGUAGES = ["en", "zh-CN"] as const;
export type Language = (typeof SUPPORTED_LANGUAGES)[number];

export const LANGUAGE_STORAGE_KEY = "token-poker.language";

export interface LanguageStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function isLanguage(value: string | null | undefined): value is Language {
  return value === "en" || value === "zh-CN";
}

export function resolveSystemLanguage(languages: readonly string[] | null | undefined): Language {
  const primaryLanguage = languages?.[0]?.trim().toLowerCase();
  return primaryLanguage?.startsWith("zh") === true ? "zh-CN" : "en";
}

export function resolveInitialLanguage(
  storedLanguage: string | null | undefined,
  systemLanguages: readonly string[] | null | undefined,
): Language {
  return isLanguage(storedLanguage) ? storedLanguage : resolveSystemLanguage(systemLanguages);
}

export function readStoredLanguage(storage: LanguageStorage | null): string | null {
  if (storage === null) return null;
  try {
    return storage.getItem(LANGUAGE_STORAGE_KEY);
  } catch {
    return null;
  }
}

export function persistLanguage(storage: LanguageStorage | null, language: Language): void {
  if (storage === null) return;
  try {
    storage.setItem(LANGUAGE_STORAGE_KEY, language);
  } catch {
    // The sandbox may disable persistence; language switching still works for
    // the current session.
  }
}
