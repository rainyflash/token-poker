import * as Dialog from "@radix-ui/react-dialog";
import { CircleHelp, X } from "lucide-react";
import { useI18n } from "../../../core/i18n/use-i18n";
import { Button } from "../../../shared/ui/button";

export function GameRulesDialog() {
  const { t } = useI18n();
  return (
    <Dialog.Root>
      <Dialog.Trigger asChild>
        <Button variant="ghost" size="icon" aria-label={t("lobby.details")} title={t("lobby.details")}>
          <CircleHelp className="size-5" strokeWidth={1.6} />
        </Button>
      </Dialog.Trigger>
      <Dialog.Portal container={globalThis.__tokenHoldemPortalRoot}>
        <Dialog.Overlay className="fixed inset-0 z-[999999] bg-black/15 backdrop-blur-[2px]" />
        <Dialog.Content className="token-poker-portal fixed left-1/2 top-1/2 z-[1000000] max-h-[calc(100dvh-32px)] w-[min(480px,calc(100vw-32px))] -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-2xl border border-[var(--line)] bg-white p-6 shadow-[var(--codex-shadow-lg)] outline-none">
          <div className="flex items-start justify-between gap-4">
            <Dialog.Title className="text-lg font-semibold">{t("lobby.details")}</Dialog.Title>
            <Dialog.Close asChild><Button variant="ghost" size="icon" aria-label={t("common.close")}><X className="size-4" /></Button></Dialog.Close>
          </div>
          <Dialog.Description className="mt-4 text-sm leading-6 text-[var(--muted)]">{t("lobby.description")}</Dialog.Description>
          <div className="mt-5 border-t border-[var(--line)] pt-5 text-sm leading-6 text-[var(--muted)]">
            <h2 className="font-medium text-[var(--ink)]">{t("lobby.securityTitle")}</h2>
            <p className="mt-2">{t("lobby.securityDescription")}</p>
            <p className="mt-4">{t("lobby.transport")}: {t("lobby.transportValue")}</p>
            <p className="mt-4">{t("lobby.usageReadOnly")}</p>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
