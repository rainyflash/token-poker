import * as Dialog from "@radix-ui/react-dialog";
import { ChevronDown, ChevronLeft, KeyRound, RefreshCw, X } from "lucide-react";
import { useI18n } from "../../core/i18n/use-i18n";
import { AccountAvatar } from "../../shared/ui/account-avatar";
import { Button } from "../../shared/ui/button";

interface AccountMenuProps {
  readonly name: string;
  readonly avatarUrl: string | null;
  readonly usageLabel: string;
  readonly usageLoading: boolean;
  readonly onIdentity: () => void;
  readonly onRefresh: () => void;
  readonly onClose: () => void;
}

export function AccountMenu({ name, avatarUrl, usageLabel, usageLoading, onIdentity, onRefresh, onClose }: AccountMenuProps) {
  const { t } = useI18n();
  return (
    <Dialog.Root>
      <Dialog.Trigger asChild>
        <button className="app-account-trigger" type="button" aria-label={t("app.openAccount")}>
          <AccountAvatar name={name} src={avatarUrl} className="app-account-avatar" />
          <span className="app-account-copy"><strong>{name}</strong><span>{usageLabel}</span></span>
          <ChevronDown className="app-account-chevron size-4 text-[var(--muted)]" strokeWidth={1.6} />
        </button>
      </Dialog.Trigger>
      <Dialog.Portal container={globalThis.__tokenHoldemPortalRoot}>
        <Dialog.Overlay className="fixed inset-0 z-[999999] bg-black/5" />
        <Dialog.Content className="token-poker-portal fixed right-4 top-[72px] z-[1000000] w-[min(320px,calc(100vw-32px))] rounded-2xl border border-[var(--line)] bg-white p-4 shadow-[var(--codex-shadow-lg)] outline-none">
          <div className="flex items-start justify-between gap-3 border-b border-[var(--line)] pb-4">
            <div className="min-w-0">
              <Dialog.Title className="truncate font-semibold">{name}</Dialog.Title>
              <Dialog.Description className="mt-1 text-sm text-[var(--muted)]">{usageLabel}</Dialog.Description>
            </div>
            <Dialog.Close asChild><Button variant="ghost" size="icon" aria-label={t("common.close")}><X className="size-4" /></Button></Dialog.Close>
          </div>
          <div className="mt-2 grid gap-1">
            <Dialog.Close asChild><Button variant="ghost" className="justify-start" onClick={onIdentity}><KeyRound className="size-4" />{t("subview.identity")}</Button></Dialog.Close>
            <Button variant="ghost" className="justify-start" disabled={usageLoading} onClick={onRefresh}>
              <RefreshCw className={usageLoading ? "size-4 animate-spin" : "size-4"} />{t("app.refreshAccount")}
            </Button>
            <Dialog.Close asChild><Button variant="ghost" className="justify-start" onClick={onClose}><ChevronLeft className="size-4" />{t("app.returnToCodex")}</Button></Dialog.Close>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
