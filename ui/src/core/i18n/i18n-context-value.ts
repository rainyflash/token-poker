import { createContext } from "react";
import type { Language } from "./language-preference";
import type { MessageKey, MessageVariables } from "./messages";

export interface I18nContextValue {
  readonly language: Language;
  readonly intlLocale: "en-US" | "zh-CN";
  readonly setLanguage: (language: Language) => void;
  readonly toggleLanguage: () => void;
  readonly t: (key: MessageKey, variables?: MessageVariables) => string;
  readonly formatTokens: (value: number) => string;
  readonly formatSignedTokens: (value: number) => string;
  readonly formatInteger: (value: number) => string;
  readonly formatDate: (value: Date | number) => string;
}

export const I18nContext = createContext<I18nContextValue | null>(null);
