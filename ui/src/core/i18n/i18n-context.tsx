import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { formatSignedTokensForLocale, formatTokensForLocale } from "../../shared/lib/tokens";
import {
  persistLanguage,
  readStoredLanguage,
  resolveInitialLanguage,
  type Language,
} from "./language-preference";
import { translate, type MessageKey, type MessageVariables } from "./messages";
import { I18nContext, type I18nContextValue } from "./i18n-context-value";

function browserStorage(): Storage | null {
  try {
    return globalThis.localStorage;
  } catch {
    return null;
  }
}

function browserLanguages(): readonly string[] {
  try {
    return globalThis.navigator.languages.length > 0
      ? globalThis.navigator.languages
      : [globalThis.navigator.language];
  } catch {
    return [];
  }
}

export function I18nProvider({ children }: { readonly children: ReactNode }) {
  const storage = browserStorage();
  const [language, setLanguageState] = useState<Language>(() =>
    resolveInitialLanguage(readStoredLanguage(storage), browserLanguages()),
  );
  const intlLocale = language === "zh-CN" ? "zh-CN" : "en-US";

  const setLanguage = useCallback(
    (nextLanguage: Language): void => {
      persistLanguage(storage, nextLanguage);
      setLanguageState(nextLanguage);
    },
    [storage],
  );
  const toggleLanguage = useCallback(
    (): void => setLanguage(language === "en" ? "zh-CN" : "en"),
    [language, setLanguage],
  );
  const t = useCallback(
    (key: MessageKey, variables?: MessageVariables): string =>
      translate(language, key, variables),
    [language],
  );
  const integerFormatter = useMemo(() => new Intl.NumberFormat(intlLocale), [intlLocale]);
  const dateFormatter = useMemo(
    () =>
      new Intl.DateTimeFormat(intlLocale, {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
      }),
    [intlLocale],
  );

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  const value = useMemo<I18nContextValue>(
    () => ({
      language,
      intlLocale,
      setLanguage,
      toggleLanguage,
      t,
      formatTokens: (amount) => formatTokensForLocale(amount, intlLocale),
      formatSignedTokens: (amount) => formatSignedTokensForLocale(amount, intlLocale),
      formatInteger: (amount) => integerFormatter.format(amount),
      formatDate: (date) => dateFormatter.format(date),
    }),
    [dateFormatter, integerFormatter, intlLocale, language, setLanguage, t, toggleLanguage],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}
