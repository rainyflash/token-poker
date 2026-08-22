import { Radar, Route } from "lucide-react";
import { useState } from "react";
import type {
  CommandResult,
  DiscoverySnapshot,
  HostCommand,
} from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";
import { Button } from "../../../shared/ui/button";
import { StatusPill } from "../../../shared/ui/status-pill";

interface DiscoveryNodeConfigProps {
  readonly discovery: DiscoverySnapshot;
  readonly sidecarReady: boolean;
  readonly sendCommand: (command: HostCommand) => CommandResult;
}

function isPeerMultiaddr(value: string): boolean {
  const length = new TextEncoder().encode(value).length;
  return value.startsWith("/") && value.includes("/p2p/") && length <= 2_048;
}

export function DiscoveryNodeConfig({
  discovery,
  sidecarReady,
  sendCommand,
}: DiscoveryNodeConfigProps) {
  const { t } = useI18n();
  const [nodesText, setNodesText] = useState("");
  const [namespace, setNamespace] = useState("token-holdem/v1");
  const [externalAddress, setExternalAddress] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const configuredNodes = discovery.nodes.length;
  const registeredNodes = discovery.registeredNodes.size;
  const networkStatus =
    configuredNodes === 0
      ? { label: t("discovery.notConfigured"), tone: "attention" as const }
      : registeredNodes === 0
        ? { label: t("discovery.connecting"), tone: "attention" as const }
        : { label: t("discovery.connected"), tone: "success" as const };

  const configure = (): void => {
    const nodes = Array.from(
      new Set(
        nodesText
          .split(/\r?\n/u)
          .map((address) => address.trim())
          .filter((address) => address.length > 0),
      ),
    );
    const normalizedNamespace = namespace.trim().toLowerCase();
    if (nodes.length < 1 || nodes.length > 8 || !nodes.every(isPeerMultiaddr)) {
      setMessage(t("discovery.invalidNodes"));
      return;
    }
    if (!/^[a-z0-9_/-]{1,64}$/u.test(normalizedNamespace)) {
      setMessage(t("discovery.invalidNamespace"));
      return;
    }
    const advertised = externalAddress.trim();
    if (advertised.length > 0) {
      const advertisedResult = sendCommand({ type: "add_external_address", address: advertised });
      if (!advertisedResult.ok) {
        setMessage(advertisedResult.error);
        return;
      }
    }
    const result = sendCommand({
      type: "configure_discovery",
      addresses: nodes,
      namespace: normalizedNamespace,
    });
    setMessage(result.ok ? t("discovery.started") : result.error);
  };

  return (
    <div className="mt-5 rounded-[16px] border border-[var(--line)] bg-white p-5">
      <div className="flex items-start justify-between gap-4">
        <div className="flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-[11px] border border-[#dce9df] bg-[#f7fbf7]">
            <Radar className="size-5 text-[#477d50]" strokeWidth={1.7} />
          </div>
          <div>
            <h2 className="text-[12px] font-semibold">{t("discovery.title")}</h2>
            <p className="mt-1 text-[9px] text-[var(--muted-light)]">
              {t("discovery.description")}
            </p>
          </div>
        </div>
        <StatusPill
          label={networkStatus.label}
          tone={networkStatus.tone}
          dot
        />
      </div>

      <div className="mt-5 grid gap-4 min-[760px]:grid-cols-[1fr_220px]">
        <label className="block text-[10px] font-medium text-[var(--muted)]">
          {t("discovery.nodes")}
          <textarea
            value={nodesText}
            onChange={(event) => setNodesText(event.target.value)}
            placeholder="/dns4/community.example/tcp/4001/p2p/12D3KooW…"
            className="mt-2 h-[92px] w-full resize-none rounded-[11px] border border-[var(--line-strong)] bg-white p-3 font-mono text-[9px] font-normal leading-4 outline-none placeholder:text-[var(--muted-light)] focus:border-[#78bdeb] focus:ring-2 focus:ring-[#cde9fb]"
          />
        </label>
        <div className="space-y-3">
          <label className="block text-[10px] font-medium text-[var(--muted)]">
            {t("discovery.namespace")}
            <input
              value={namespace}
              onChange={(event) => setNamespace(event.target.value)}
              className="mt-2 h-10 w-full rounded-[10px] border border-[var(--line-strong)] px-3 font-mono text-[9px] font-normal outline-none focus:border-[#78bdeb] focus:ring-2 focus:ring-[#cde9fb]"
            />
          </label>
          <p className="text-[9px] leading-4 text-[var(--muted-light)]">
            {registeredNodes > 0
              ? discovery.lastDiscoveredPeers === 0
                ? t("discovery.noPeers")
                : t("discovery.peerCount", { count: discovery.lastDiscoveredPeers })
              : configuredNodes > 0
                ? t("discovery.reserving")
                : t("discovery.notLoaded")}
          </p>
        </div>
      </div>

      <div className="mt-4 flex flex-col gap-3 border-t border-[var(--line)] pt-4 min-[680px]:flex-row min-[680px]:items-end">
        <label className="min-w-0 flex-1 text-[10px] font-medium text-[var(--muted)]">
          {t("discovery.publicAddress")}
          <input
            value={externalAddress}
            onChange={(event) => setExternalAddress(event.target.value)}
            placeholder="/ip4/203.0.113.10/tcp/4201"
            className="mt-2 h-10 w-full rounded-[10px] border border-[var(--line-strong)] px-3 font-mono text-[9px] font-normal outline-none placeholder:text-[var(--muted-light)] focus:border-[#78bdeb] focus:ring-2 focus:ring-[#cde9fb]"
          />
        </label>
        <Button disabled={!sidecarReady} onClick={configure}>
          <Route className="size-4" />
          {t("discovery.enable")}
        </Button>
      </div>
      {message ? <p className="mt-3 text-[9px] leading-4 text-[var(--muted)]">{message}</p> : null}
    </div>
  );
}
