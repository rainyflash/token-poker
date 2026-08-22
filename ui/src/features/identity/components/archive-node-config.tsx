import { CloudCog, RadioTower, ServerCog } from "lucide-react";
import { useState } from "react";
import type {
  ArchiveSnapshot,
  CommandResult,
  HostCommand,
} from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";
import { Button } from "../../../shared/ui/button";
import { StatusPill } from "../../../shared/ui/status-pill";

interface ArchiveNodeConfigProps {
  readonly archive: ArchiveSnapshot;
  readonly sidecarReady: boolean;
  readonly sendCommand: (command: HostCommand) => CommandResult;
}

function isPeerMultiaddr(value: string): boolean {
  const encodedLength = new TextEncoder().encode(value).length;
  return value.startsWith("/") && value.includes("/p2p/") && encodedLength <= 2_048;
}

export function ArchiveNodeConfig({
  archive,
  sidecarReady,
  sendCommand,
}: ArchiveNodeConfigProps) {
  const { t } = useI18n();
  const [addressesText, setAddressesText] = useState("");
  const [replicas, setReplicas] = useState("1");
  const [relayAddress, setRelayAddress] = useState("");
  const [message, setMessage] = useState<string | null>(null);

  const configureArchives = (): void => {
    const addresses = Array.from(
      new Set(
        addressesText
          .split(/\r?\n/u)
          .map((address) => address.trim())
          .filter((address) => address.length > 0),
      ),
    );
    const required = Number.parseInt(replicas, 10);
    if (addresses.length < 1 || addresses.length > 16 || !addresses.every(isPeerMultiaddr)) {
      setMessage(t("archive.invalidAddresses"));
      return;
    }
    if (!Number.isInteger(required) || required < 1 || required > addresses.length) {
      setMessage(t("archive.invalidReplicas"));
      return;
    }
    const result = sendCommand({
      type: "configure_archive_nodes",
      addresses,
      minimum_confirmed_replicas: required,
    });
    setMessage(result.ok ? t("archive.connecting") : result.error);
  };

  const connectRelay = (): void => {
    const address = relayAddress.trim();
    if (!isPeerMultiaddr(address)) {
      setMessage(t("archive.invalidRelay"));
      return;
    }
    const result = sendCommand({ type: "use_relay", address });
    setMessage(result.ok ? t("archive.relayRequested") : result.error);
  };

  return (
    <div className="mt-5 rounded-[16px] border border-[var(--line)] bg-white p-5">
      <div className="flex items-start justify-between gap-4">
        <div className="flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-[11px] border border-black/[.06] bg-[var(--surface-subtle)]">
            <CloudCog className="size-5 text-[var(--muted)]" strokeWidth={1.7} />
          </div>
          <div>
            <h2 className="text-[12px] font-semibold">{t("archive.title")}</h2>
            <p className="mt-1 text-[9px] text-[var(--muted-light)]">
              {t("archive.description")}
            </p>
          </div>
        </div>
        <StatusPill
          label={archive.peers.length > 0 ? t("archive.nodeCount", { count: archive.peers.length }) : t("common.notConfigured")}
          tone={archive.peers.length > 0 ? "success" : "attention"}
          dot
        />
      </div>

      <div className="mt-5 grid gap-4 min-[760px]:grid-cols-[1fr_220px]">
        <label className="block text-[10px] font-medium text-[var(--muted)]">
          {t("archive.addresses")}
          <textarea
            value={addressesText}
            onChange={(event) => setAddressesText(event.target.value)}
            placeholder="/dns4/archive.example/tcp/443/wss/p2p/12D3KooW…"
            className="mt-2 h-[92px] w-full resize-none rounded-[11px] border border-[var(--line-strong)] bg-white p-3 font-mono text-[9px] font-normal leading-4 outline-none placeholder:text-[var(--muted-light)] focus:border-[#78bdeb] focus:ring-2 focus:ring-[#cde9fb]"
          />
        </label>
        <div>
          <label className="block text-[10px] font-medium text-[var(--muted)]">
            {t("archive.minimumReplicas")}
            <input
              inputMode="numeric"
              value={replicas}
              onChange={(event) => setReplicas(event.target.value)}
              className="mt-2 h-10 w-full rounded-[10px] border border-[var(--line-strong)] px-3 text-[11px] font-normal outline-none focus:border-[#78bdeb] focus:ring-2 focus:ring-[#cde9fb]"
            />
          </label>
          <Button
            className="mt-3 w-full"
            disabled={!sidecarReady}
            onClick={configureArchives}
          >
            <ServerCog className="size-4" />
            {t("archive.configure")}
          </Button>
        </div>
      </div>

      <div className="mt-4 flex flex-col gap-3 border-t border-[var(--line)] pt-4 min-[680px]:flex-row min-[680px]:items-end">
        <label className="min-w-0 flex-1 text-[10px] font-medium text-[var(--muted)]">
          {t("archive.optionalRelay")}
          <input
            value={relayAddress}
            onChange={(event) => setRelayAddress(event.target.value)}
            placeholder="/dns4/relay.example/tcp/443/wss/p2p/12D3KooW…"
            className="mt-2 h-10 w-full rounded-[10px] border border-[var(--line-strong)] px-3 font-mono text-[9px] font-normal outline-none placeholder:text-[var(--muted-light)] focus:border-[#78bdeb] focus:ring-2 focus:ring-[#cde9fb]"
          />
        </label>
        <Button disabled={!sidecarReady || relayAddress.trim().length === 0} onClick={connectRelay}>
          <RadioTower className="size-4" />
          {t("archive.requestRelay")}
        </Button>
      </div>
      {message ? <p className="mt-3 text-[9px] leading-4 text-[var(--muted)]">{message}</p> : null}
    </div>
  );
}
