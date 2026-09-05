import * as Dialog from "@radix-ui/react-dialog";
import { Check, Copy, DoorOpen, Link2, X } from "lucide-react";
import { useState } from "react";
import type {
  CommandResult,
  FriendRoomStatus,
  HostCommand,
} from "../../../core/bridge/contracts";
import { useI18n } from "../../../core/i18n/use-i18n";
import { Button } from "../../../shared/ui/button";
import type { StakeLevel } from "../model/stake-levels";

interface FriendRoomDialogProps {
  readonly level: StakeLevel;
  readonly buyIn: number;
  readonly inviteCode: string | null;
  readonly identityReady: boolean;
  readonly roomStatus: FriendRoomStatus;
  readonly sendCommand: (command: HostCommand) => CommandResult;
}

export function FriendRoomDialog({
  level,
  buyIn,
  inviteCode,
  identityReady,
  roomStatus,
  sendCommand,
}: FriendRoomDialogProps) {
  const { t } = useI18n();
  const [tab, setTab] = useState<"create" | "join">("create");
  const [joinCode, setJoinCode] = useState("");
  const [copied, setCopied] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const createRoom = (): void => {
    if (!identityReady) {
      setMessage(t("friend.identityRequired"));
      return;
    }
    const result = sendCommand({
      type: "create_friend_room",
      level_id: level.id,
      buy_in: buyIn,
    });
    setMessage(result.ok ? t("friend.signing") : result.error);
  };

  const joinRoom = (): void => {
    if (!identityReady) {
      setMessage(t("friend.identityRequired"));
      return;
    }
    const normalized = joinCode.trim();
    if (normalized.length < 12) {
      setMessage(t("friend.codeTooShort"));
      return;
    }
    const result = sendCommand({ type: "join_friend_room", invite_code: normalized, buy_in: buyIn });
    setMessage(result.ok ? t("friend.connecting") : result.error);
  };

  const copyInvite = async (): Promise<void> => {
    if (inviteCode === null) return;
    try {
      await navigator.clipboard.writeText(inviteCode);
      setCopied(true);
      globalThis.setTimeout(() => setCopied(false), 1_500);
    } catch (error: unknown) {
      setMessage(
        error instanceof Error
          ? t("friend.copyFailed", { reason: error.message })
          : t("friend.copyFailedGeneric"),
      );
    }
  };

  return (
    <Dialog.Root>
      <Dialog.Trigger asChild>
        <Button size="lg" className="w-full rounded-full">
          <DoorOpen className="size-4" />
          {t("friend.open")}
        </Button>
      </Dialog.Trigger>
      <Dialog.Portal container={globalThis.__tokenHoldemPortalRoot}>
        <Dialog.Overlay className="fixed inset-0 z-[999999] bg-black/15 backdrop-blur-[2px] data-[state=open]:animate-in" />
        <Dialog.Content className="token-poker-portal fixed left-1/2 top-1/2 z-[1000000] max-h-[calc(100dvh-32px)] w-[min(440px,calc(100%-32px))] -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-[18px] border border-black/[.10] bg-white p-5 text-[var(--ink)] shadow-[0_30px_90px_rgba(18,24,20,.22)] outline-none">
          <div className="flex items-start justify-between">
            <div>
              <Dialog.Title className="text-[18px] font-semibold tracking-[-0.035em]">{t("friend.title")}</Dialog.Title>
              <Dialog.Description className="mt-2 text-[13px] leading-5 text-[var(--muted)]">
                {t("friend.description")}
              </Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <Button variant="ghost" size="icon" className="size-8" aria-label={t("common.close")}>
                <X className="size-4" />
              </Button>
            </Dialog.Close>
          </div>

          <div className="mt-5 grid grid-cols-2 rounded-[10px] bg-[#f3f5f3] p-1">
            {(["create", "join"] as const).map((value) => (
              <button
                key={value}
                type="button"
                aria-pressed={tab === value}
                className={`h-10 rounded-[8px] text-[13px] font-medium outline-none transition-colors focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] ${
                  tab === value ? "bg-white text-[var(--ink)] shadow-sm" : "text-[var(--muted)]"
                }`}
                onClick={() => {
                  setTab(value);
                  setMessage(null);
                }}
              >
                {t(value === "create" ? "friend.create" : "friend.join")}
              </button>
            ))}
          </div>

          {tab === "create" ? (
            <div className="mt-5">
              <div className="grid grid-cols-3 gap-2 rounded-[11px] border border-[var(--line)] bg-[var(--surface-subtle)] p-3 text-center">
                <div>
                  <p className="text-xs text-[var(--muted)]">{t("friend.level")}</p>
                  <p className="mt-1 text-[13px] font-medium">{t(level.nameKey)}</p>
                </div>
                <div className="border-x border-black/[.055]">
                  <p className="text-xs text-[var(--muted)]">{t("friend.seats")}</p>
                  <p className="mt-1 text-[13px] font-medium">{t("friend.dynamicSeats")}</p>
                </div>
                <div>
                  <p className="text-xs text-[var(--muted)]">{t("friend.expiry")}</p>
                  <p className="mt-1 text-[13px] font-medium">{t("friend.thirtyMinutes")}</p>
                </div>
              </div>
              {inviteCode ? (
                <button
                  type="button"
                  onClick={() => void copyInvite()}
                  className="mt-4 flex w-full items-center justify-between rounded-[11px] border border-[#bdddf3] bg-[#f5faff] px-4 py-3 text-left"
                >
                  <span>
                    <span className="block text-xs text-[#5f86a0]">{t("friend.signedCode")}</span>
                    <code className="mt-1 block max-w-[330px] truncate font-mono text-[13px] font-semibold text-[#155f91]">
                      {inviteCode}
                    </code>
                  </span>
                  {copied ? <Check className="size-4 text-[#2f8a45]" /> : <Copy className="size-4 text-[#397da9]" />}
                </button>
              ) : (
                <Button variant="primary" size="lg" className="mt-4 w-full" onClick={createRoom}>
                  <Link2 className="size-4" />
                  {t("friend.generate")}
                </Button>
              )}
            </div>
          ) : (
            <div className="mt-5">
              <label className="text-xs font-medium text-[var(--muted)]" htmlFor="friend-invite">
                {t("friend.inviteCode")}
              </label>
              <textarea
                id="friend-invite"
                value={joinCode}
                onChange={(event) => setJoinCode(event.target.value)}
                placeholder="TH1-…"
                className="mt-2 h-24 w-full resize-none rounded-[11px] border border-[var(--line-strong)] bg-white p-3 font-mono text-[13px] outline-none placeholder:text-[var(--muted)] focus:border-[#78bdeb] focus:ring-2 focus:ring-[#cde9fb]"
              />
              <Button variant="primary" size="lg" className="mt-3 w-full" onClick={joinRoom}>
                {t("friend.verifyJoin")}
              </Button>
            </div>
          )}
          {message || roomStatus !== "idle" ? (
            <p className="mt-3 text-xs leading-4 text-[var(--muted)]">
              {roomStatus === "joining"
                ? t("friend.statusJoining")
                : roomStatus === "joined"
                  ? t("friend.statusJoined")
                  : roomStatus === "created"
                    ? t("friend.statusCreated")
                    : message}
            </p>
          ) : null}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
