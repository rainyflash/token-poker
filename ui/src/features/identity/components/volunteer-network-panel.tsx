import {
  BatteryCharging,
  CircleDot,
  Gauge,
  Network,
  Power,
  RadioTower,
  Router,
  Server,
  ShieldCheck,
  Wifi,
} from "lucide-react";
import { motion } from "motion/react";
import { useState } from "react";
import type {
  CommandResult,
  HostCommand,
  VolunteerSnapshot,
} from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";
import type { MessageKey, MessageVariables } from "../../../core/i18n/messages";
import { Button } from "../../../shared/ui/button";
import { StatusPill } from "../../../shared/ui/status-pill";

interface VolunteerNetworkPanelProps {
  readonly volunteer: VolunteerSnapshot;
  readonly sidecarReady: boolean;
  readonly sendCommand: (command: HostCommand) => CommandResult;
}

const ROLE_LABELS: Record<VolunteerSnapshot["role"], MessageKey> = {
  disabled: "volunteer.roleDisabled",
  discovery_candidate: "volunteer.roleDiscoveryCandidate",
  relay_candidate: "volunteer.roleRelayCandidate",
  active_discovery: "volunteer.roleActiveDiscovery",
  active_discovery_relay: "volunteer.roleActiveRelay",
};

const ROLE_DESCRIPTIONS: Record<VolunteerSnapshot["role"], MessageKey> = {
  disabled: "volunteer.descDisabled",
  discovery_candidate: "volunteer.descDiscoveryCandidate",
  relay_candidate: "volunteer.descRelayCandidate",
  active_discovery: "volunteer.descActiveDiscovery",
  active_discovery_relay: "volunteer.descActiveRelay",
};

const REASON_LABELS: Record<VolunteerSnapshot["policyReason"], MessageKey> = {
  eligible: "volunteer.reasonEligible",
  consent_required: "volunteer.reasonConsent",
  declined: "volunteer.reasonDeclined",
  metered_network: "volunteer.reasonMetered",
  battery_power: "volunteer.reasonBattery",
  host_conditions_unknown: "volunteer.reasonUnknown",
};

const NETWORK_LABELS: Record<VolunteerSnapshot["networkCost"], MessageKey> = {
  unmetered: "volunteer.networkUnmetered",
  metered: "volunteer.networkMetered",
  unknown: "volunteer.networkUnknown",
};

const POWER_LABELS: Record<VolunteerSnapshot["powerSource"], MessageKey> = {
  ac: "volunteer.powerAc",
  battery: "volunteer.powerBattery",
  unknown: "volunteer.powerUnknown",
};

const REACHABILITY_LABELS: Record<VolunteerSnapshot["reachability"], MessageKey> = {
  unknown: "volunteer.reachUnknown",
  private: "volunteer.reachPrivate",
  public: "volunteer.reachPublic",
};

function formatBytes(value: number, formatInteger: (value: number) => string): string {
  return `${formatInteger(Math.round(value / 1_048_576))} MiB`;
}

function formatDuration(
  seconds: number,
  t: (key: MessageKey, variables?: MessageVariables) => string,
  formatInteger: (value: number) => string,
): string {
  const hours = seconds / 3_600;
  return Number.isInteger(hours)
    ? t("common.hours", { count: formatInteger(hours) })
    : t("common.seconds", { count: formatInteger(seconds) });
}

export function VolunteerNetworkPanel({
  volunteer,
  sidecarReady,
  sendCommand,
}: VolunteerNetworkPanelProps) {
  const { t, formatInteger } = useI18n();
  const [message, setMessage] = useState<string | null>(null);
  const isGranted = volunteer.consent === "granted";
  const isUndecided = volunteer.consent === "undecided";
  const isActive = volunteer.role === "active_discovery_relay";

  const updateConsent = (enabled: boolean): void => {
    const result = sendCommand({ type: "set_volunteer_consent", enabled });
    setMessage(
      result.ok
        ? enabled
          ? t("volunteer.enabled")
          : t("volunteer.disabled")
        : result.error,
    );
  };

  return (
    <motion.section
      aria-labelledby="volunteer-network-title"
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ type: "spring", stiffness: 320, damping: 30 }}
      className="mt-5 overflow-hidden rounded-[16px] border border-[var(--line)] bg-white shadow-[0_1px_2px_rgba(20,24,21,.025)]"
    >
      <div className="grid min-[900px]:grid-cols-[1.12fr_.88fr]">
        <div className="p-5 min-[760px]:p-6">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div className="flex items-center gap-3">
              <div className="grid size-10 place-items-center rounded-[11px] border border-[#d6e8f8] bg-[#f5faff]">
                <Network className="size-5 text-[#246a99]" strokeWidth={1.7} />
              </div>
              <div>
                <p className="text-[9px] font-semibold uppercase tracking-[.14em] text-[var(--muted-light)]">
                  {t("volunteer.eyebrow")}
                </p>
                <h2 id="volunteer-network-title" className="mt-1 text-[13px] font-semibold">
                  {t("volunteer.title")}
                </h2>
              </div>
            </div>
            <StatusPill
              icon={isActive ? RadioTower : CircleDot}
              label={t(ROLE_LABELS[volunteer.role])}
              tone={isActive ? "success" : isGranted ? "info" : "neutral"}
            />
          </div>

          <p className="mt-5 max-w-[610px] text-[11px] leading-[1.75] text-[var(--muted)]">
            {t("volunteer.description")}
          </p>

          <div className="mt-5 grid grid-cols-1 gap-2.5 sm:grid-cols-3">
            <Condition
              icon={Wifi}
              label={t("volunteer.networkCost")}
              value={t(NETWORK_LABELS[volunteer.networkCost])}
              healthy={volunteer.networkCost === "unmetered"}
            />
            <Condition
              icon={Power}
              label={t("volunteer.powerStatus")}
              value={t(POWER_LABELS[volunteer.powerSource])}
              healthy={volunteer.powerSource === "ac"}
            />
            <Condition
              icon={Router}
              label={t("volunteer.publicCapability")}
              value={t(REACHABILITY_LABELS[volunteer.reachability])}
              healthy={volunteer.reachability === "public"}
            />
          </div>

          <div className="mt-5 flex flex-wrap items-center gap-2.5 border-t border-[var(--line)] pt-5">
            <Button
              variant={isGranted ? "danger" : "primary"}
              disabled={!sidecarReady && !volunteer.restartRequired}
              onClick={() => updateConsent(!isGranted)}
            >
              {isGranted ? <Power className="size-4" /> : <ShieldCheck className="size-4" />}
              {t(isGranted ? "volunteer.stop" : "volunteer.authorize")}
            </Button>
            {isUndecided ? (
              <Button variant="ghost" onClick={() => updateConsent(false)}>
                {t("volunteer.notNow")}
              </Button>
            ) : null}
            <span className="text-[9px] text-[var(--muted-light)]">
              {t(REASON_LABELS[volunteer.policyReason])}
            </span>
          </div>

          {volunteer.restartRequired ? (
            <p className="mt-3 rounded-[9px] border border-[#f0dfbd] bg-[#fffbf2] px-3 py-2 text-[9px] leading-4 text-[#8b621c]">
              {t("volunteer.restartDeferred")}
            </p>
          ) : null}
          {message ? (
            <p aria-live="polite" className="mt-3 text-[9px] leading-4 text-[var(--muted)]">
              {message}
            </p>
          ) : null}
        </div>

        <div className="border-t border-[var(--line)] bg-[#f7f8f6] p-5 min-[900px]:border-l min-[900px]:border-t-0 min-[760px]:p-6">
          <p className="text-[9px] font-semibold uppercase tracking-[.14em] text-[var(--muted-light)]">
            {t("volunteer.liveEyebrow")}
          </p>
          <h3 className="mt-2 text-[17px] font-semibold tracking-[-.025em]">
            {t(ROLE_LABELS[volunteer.role])}
          </h3>
          <p className="mt-2 min-h-10 text-[10px] leading-[1.65] text-[var(--muted)]">
            {t(ROLE_DESCRIPTIONS[volunteer.role])}
          </p>

          <div className="mt-5 grid grid-cols-2 overflow-hidden rounded-[11px] border border-[var(--line)] bg-white">
            <Metric
              icon={Server}
              label={t("volunteer.discoveryService")}
              value={t(volunteer.discoveryServerEnabled ? "volunteer.loaded" : "volunteer.off")}
            />
            <Metric
              icon={RadioTower}
              label={t("volunteer.relayService")}
              value={t(volunteer.relayServerEnabled ? "volunteer.loaded" : "volunteer.off")}
            />
            <Metric
              icon={BatteryCharging}
              label={t("volunteer.reservations")}
              value={`${String(volunteer.activeReservations)} / ${String(volunteer.maxReservations)}`}
            />
            <Metric
              icon={Gauge}
              label={t("volunteer.activeCircuits")}
              value={`${String(volunteer.activeCircuits)} / ${String(volunteer.maxCircuits)}`}
            />
          </div>

          <div className="mt-4 space-y-2 text-[9px] text-[var(--muted)]">
            <div className="flex items-center justify-between gap-4">
              <span>{t("volunteer.circuitLimit")}</span>
              <span className="font-medium text-[var(--ink)]">
                {formatDuration(volunteer.maxCircuitDurationSeconds, t, formatInteger)} · {formatBytes(volunteer.maxCircuitBytes, formatInteger)}
              </span>
            </div>
            <div className="flex items-center justify-between gap-4">
              <span>{t("volunteer.directory")}</span>
              <span className="font-medium text-[var(--ink)]">
                {t("volunteer.directoryValue", { discovery: formatInteger(volunteer.directoryRendezvousNodes), relay: formatInteger(volunteer.directoryRelayNodes) })}
              </span>
            </div>
            <div className="flex items-center justify-between gap-4">
              <span>{t("volunteer.coldStart")}</span>
              <span className="font-medium text-[var(--ink)]">
                {t(volunteer.coldStartAvailable ? "volunteer.available" : "volunteer.awaitingNode")}
              </span>
            </div>
          </div>
        </div>
      </div>
    </motion.section>
  );
}

interface DetailProps {
  readonly icon: typeof Wifi;
  readonly label: string;
  readonly value: string;
}

function Condition({ icon: Icon, label, value, healthy }: DetailProps & { readonly healthy: boolean }) {
  return (
    <div className="rounded-[11px] border border-[var(--line)] bg-[#fafbf9] px-3.5 py-3">
      <div className="flex items-center gap-2 text-[9px] text-[var(--muted-light)]">
        <Icon className="size-3.5" strokeWidth={1.7} />
        {label}
      </div>
      <p className={`mt-2 text-[10px] font-medium ${healthy ? "text-[#3c6941]" : "text-[var(--ink)]"}`}>
        {value}
      </p>
    </div>
  );
}

function Metric({ icon: Icon, label, value }: DetailProps) {
  return (
    <div className="border-b border-r border-[var(--line)] p-3.5 even:border-r-0 [&:nth-last-child(-n+2)]:border-b-0">
      <div className="flex items-center gap-2 text-[9px] text-[var(--muted-light)]">
        <Icon className="size-3.5" strokeWidth={1.7} />
        {label}
      </div>
      <p className="mt-2 text-[11px] font-semibold">{value}</p>
    </div>
  );
}
