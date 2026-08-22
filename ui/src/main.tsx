import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import { I18nProvider } from "./core/i18n/i18n-context";
import { CODEX_MARK_DATA_URI } from "./shared/assets/codex-mark";
import "./styles.css";

globalThis.__tokenHoldemCodexMarkSource = CODEX_MARK_DATA_URI;

const mountNode = globalThis.__tokenHoldemMountRoot ?? document.getElementById("root");
if (!(mountNode instanceof HTMLElement)) {
  throw new Error("Token Poker requires a valid mount element");
}

createRoot(mountNode, { identifierPrefix: "token-holdem" }).render(
  <StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </StrictMode>,
);
