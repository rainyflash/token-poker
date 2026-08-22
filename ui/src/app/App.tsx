import * as Tooltip from "@radix-ui/react-tooltip";
import { useState, type ReactNode } from "react";
import { useHostBridge } from "../core/bridge/host-bridge";
import { IdentityView } from "../features/identity/identity-view";
import { useAutoIdentity } from "../features/identity/model/use-auto-identity";
import { LobbyView } from "../features/lobby/lobby-view";
import { MatchmakingView } from "../features/lobby/matchmaking-view";
import { StatisticsView } from "../features/statistics/statistics-view";
import { TableView } from "../features/table/table-view";
import { AppShell } from "./app-shell";
import {
  projectPrimarySurface,
  type AppSubview,
  type PrimarySurface,
} from "./app-state";
import { SubviewLayer } from "./subview-layer";

export function App() {
  const [subviewState, setSubviewState] = useState<{
    readonly surface: PrimarySurface;
    readonly subview: AppSubview;
  } | null>(null);
  const [bridge, sendCommand] = useHostBridge();
  const autoIdentity = useAutoIdentity(bridge, sendCommand);
  const primarySurface = projectPrimarySurface(bridge);
  const activeSubview =
    subviewState?.surface === primarySurface ? subviewState.subview : null;
  const openSubview = (subview: AppSubview): void => {
    setSubviewState({ surface: primarySurface, subview });
  };

  const close = (): void => {
    const result = sendCommand({ type: "close_ui" });
    if (!result.ok) {
      document.getElementById("token-holdem-host")?.remove();
    }
  };

  let primaryContent: ReactNode;
  switch (primarySurface) {
    case "table":
      primaryContent = (
        <TableView
          bridge={bridge}
          sendCommand={sendCommand}
          onOpenStatistics={() => openSubview("statistics")}
        />
      );
      break;
    case "lobby":
      primaryContent = <LobbyView bridge={bridge} sendCommand={sendCommand} />;
      break;
    case "matching":
      primaryContent = <MatchmakingView bridge={bridge} sendCommand={sendCommand} />;
      break;
  }

  const subviewContent =
    activeSubview === "statistics" ? (
      <StatisticsView bridge={bridge} sendCommand={sendCommand} />
    ) : activeSubview === "identity" ? (
      <IdentityView
        bridge={bridge}
        recoveryKit={autoIdentity.recoveryKit}
        autoIdentityError={autoIdentity.error}
        sendCommand={sendCommand}
      />
    ) : null;

  return (
    <Tooltip.Provider delayDuration={360}>
      <AppShell
        primarySurface={primarySurface}
        activeSubview={activeSubview}
        bridge={bridge}
        onOpenSubview={openSubview}
        onSyncToken={() => sendCommand({ type: "request_token_refresh" })}
        onClose={close}
      >
        {primaryContent}
        {activeSubview !== null && subviewContent !== null ? (
          <SubviewLayer subview={activeSubview} onClose={() => setSubviewState(null)}>
            {subviewContent}
          </SubviewLayer>
        ) : null}
      </AppShell>
    </Tooltip.Provider>
  );
}
