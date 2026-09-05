import * as Dialog from "@radix-ui/react-dialog";
import {
  ArrowDownToLine,
  Check,
  CircleArrowUp,
  LoaderCircle,
  RefreshCw,
  ShieldCheck,
  TriangleAlert,
  X,
} from "lucide-react";
import { useState } from "react";
import type {
  CommandResult,
  HostCommand,
  UpdatePhase,
  UpdateStatus,
} from "../../core/bridge/contracts";
import { useI18n } from "../../core/i18n/use-i18n";
import type { MessageKey } from "../../core/i18n/messages";
import { Button } from "../../shared/ui/button";

const STATUS_KEYS: Readonly<Record<UpdatePhase, MessageKey>> = {
  idle: "update.statusIdle",
  checking: "update.statusChecking",
  current: "update.statusCurrent",
  available: "update.statusAvailable",
  downloading: "update.statusDownloading",
  ready: "update.statusReady",
  installing: "update.statusInstalling",
  restart_required: "update.statusRestart",
  error: "update.statusError",
};

interface UpdateDialogProps {
  readonly status: UpdateStatus;
  readonly sendCommand: (command: HostCommand) => CommandResult;
}

export function UpdateDialog({ status, sendCommand }: UpdateDialogProps) {
  const { t } = useI18n();
  const [confirming, setConfirming] = useState(false);
  const isPending = ["checking", "downloading", "installing"].includes(status.phase);
  const hasBadge = status.phase === "available" || status.phase === "ready";
  const statusIcon = status.phase === "error"
    ? <TriangleAlert className="size-4 text-[var(--codex-red-500)]" strokeWidth={1.7} />
    : status.sha256Verified
      ? <ShieldCheck className="size-4 text-[var(--codex-green-500)]" strokeWidth={1.7} />
      : status.phase === "current"
        ? <Check className="size-4 text-[var(--codex-green-500)]" strokeWidth={1.8} />
        : isPending
          ? <LoaderCircle className="size-4 animate-spin text-[var(--codex-blue-500)]" strokeWidth={1.7} />
          : <CircleArrowUp className="size-4 text-[var(--codex-blue-500)]" strokeWidth={1.7} />;

  const submit = (type: "check_update" | "prepare_update" | "install_update"): void => {
    setConfirming(false);
    sendCommand({ type });
  };

  return (
    <Dialog.Root onOpenChange={(open) => !open && setConfirming(false)}>
      <Dialog.Trigger asChild>
        <Button variant="ghost" size="icon" className="relative" aria-label={t("app.openUpdates")}>
          <CircleArrowUp className="size-4" strokeWidth={1.7} />
          {hasBadge ? (
            <span className="absolute right-1.5 top-1.5 size-1.5 rounded-full bg-[var(--codex-blue-400)] ring-2 ring-[var(--rail)]" />
          ) : null}
        </Button>
      </Dialog.Trigger>
      <Dialog.Portal container={globalThis.__tokenHoldemPortalRoot}>
        <Dialog.Overlay className="fixed inset-0 z-[999999] bg-black/15 backdrop-blur-[2px]" />
        <Dialog.Content className="token-poker-portal fixed left-1/2 top-1/2 z-[1000000] max-h-[calc(100dvh-32px)] w-[min(440px,calc(100vw-32px))] -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-[18px] border border-black/[.10] bg-white p-5 text-[var(--ink)] shadow-[0_30px_90px_rgba(18,24,20,.22)] outline-none">
          <div className="flex items-start gap-3">
            <div className="grid size-9 shrink-0 place-items-center rounded-[10px] border border-[var(--line)] bg-[var(--surface-subtle)]">
              <CircleArrowUp className="size-[17px]" strokeWidth={1.7} />
            </div>
            <div className="min-w-0 flex-1">
              <Dialog.Title className="text-[17px] font-semibold tracking-[-0.035em]">
                {t("update.title")}
              </Dialog.Title>
              <Dialog.Description className="mt-1 text-[10px] leading-4 text-[var(--muted)]">
                {t("update.description")}
              </Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <Button variant="ghost" size="icon" className="size-8" aria-label={t("common.close")}>
                <X className="size-4" />
              </Button>
            </Dialog.Close>
          </div>

          <div className="mt-5 grid grid-cols-2 overflow-hidden rounded-[11px] border border-[var(--line)] bg-[var(--surface-subtle)]">
            <VersionCell label={t("update.currentVersion")} version={status.currentVersion} />
            <VersionCell
              label={t("update.latestVersion")}
              version={status.latestVersion ?? "—"}
              className="border-l border-[var(--line)]"
            />
          </div>

          <div className="mt-3 rounded-[11px] border border-[var(--line)] px-3.5 py-3">
            <div className="flex items-start gap-2.5">
              <div className="mt-0.5">{statusIcon}</div>
              <div className="min-w-0">
                <p className="text-[11px] font-medium leading-4">{t(STATUS_KEYS[status.phase])}</p>
                {status.error ? (
                  <p className="mt-1 break-words font-mono text-[9px] leading-4 text-[var(--codex-red-500)]">
                    {status.error}
                  </p>
                ) : null}
                {status.artifactBytes !== null ? (
                  <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[9px] text-[var(--muted-light)]">
                    <span>{t("update.packageSize", { size: formatBytes(status.artifactBytes) })}</span>
                    {status.sha256Verified ? <span>{t("update.integrity")}</span> : null}
                  </div>
                ) : null}
              </div>
            </div>
          </div>

          {confirming ? (
            <div className="mt-3 rounded-[11px] border border-[color-mix(in_oklab,var(--codex-orange-500)_22%,transparent)] bg-[var(--codex-orange-25)] px-3.5 py-3">
              <p className="text-[11px] font-semibold">{t("update.confirmTitle")}</p>
              <p className="mt-1 text-[9px] leading-4 text-[var(--muted)]">
                {t("update.confirmDescription")}
              </p>
            </div>
          ) : (
            <p className="mt-3 text-[9px] leading-4 text-[var(--muted-light)]">
              {t("update.security")}
            </p>
          )}

          <div className="mt-4 flex justify-end gap-2">
            {confirming ? (
              <>
                <Button size="sm" onClick={() => setConfirming(false)}>{t("update.cancel")}</Button>
                <Button variant="primary" size="sm" onClick={() => submit("install_update")}>
                  {t("update.confirm")}
                </Button>
              </>
            ) : status.phase === "available" ? (
              <Button variant="primary" size="sm" onClick={() => submit("prepare_update")}>
                <ArrowDownToLine className="size-3.5" />
                {t("update.download")}
              </Button>
            ) : status.phase === "ready" ? (
              <Button variant="primary" size="sm" onClick={() => setConfirming(true)}>
                <ShieldCheck className="size-3.5" />
                {t("update.install")}
              </Button>
            ) : status.phase !== "restart_required" ? (
              <Button size="sm" disabled={isPending} onClick={() => submit("check_update")}>
                <RefreshCw className={`size-3.5 ${isPending ? "animate-spin" : ""}`} />
                {t("update.check")}
              </Button>
            ) : null}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function VersionCell({
  label,
  version,
  className = "",
}: {
  readonly label: string;
  readonly version: string;
  readonly className?: string;
}) {
  return (
    <div className={`px-3.5 py-3 ${className}`}>
      <p className="text-[9px] text-[var(--muted-light)]">{label}</p>
      <p className="mt-1 font-mono text-[12px] font-semibold">
        {/^[0-9]+\.[0-9]+\.[0-9]+$/u.test(version) ? `v${version}` : version}
      </p>
    </div>
  );
}

function formatBytes(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}
