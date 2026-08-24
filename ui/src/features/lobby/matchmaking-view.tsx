import { motion } from "motion/react";
import { CircleCheck, Clock3, LogOut, Radio, ShieldCheck, Users, X } from "lucide-react";
import type { CSSProperties } from "react";
import type {
  BridgeSnapshot,
  CommandResult,
  ConfirmedHostCommandSender,
  HostCommand,
  LocalRoomRole,
  PublicPoolStatus,
} from "../../core/bridge/contracts";
import { useI18n } from "../../core/i18n/use-i18n";
import type { MessageKey } from "../../core/i18n/messages";
import { projectPlayerProfile } from "../account/model/player-profile";
import { Button } from "../../shared/ui/button";
import { CodexMark } from "../../shared/ui/codex-mark";
import { StatusPill } from "../../shared/ui/status-pill";
import { useSafeLeave } from "../session/model/use-safe-leave";
import { STAKE_LEVELS } from "./model/stake-levels";

interface MatchmakingViewProps {
  readonly bridge: BridgeSnapshot;
  readonly sendCommand: (command: HostCommand) => CommandResult;
  readonly sendConfirmedCommand: ConfirmedHostCommandSender;
}

interface StatusCopy {
  readonly title: MessageKey;
  readonly detail: MessageKey;
}

const POOL_COPY: Readonly<Record<PublicPoolStatus, StatusCopy>> = {
  idle: { title: "match.poolIdleTitle", detail: "match.poolIdleDetail" },
  searching: { title: "match.poolSearchingTitle", detail: "match.poolSearchingDetail" },
  joining: { title: "match.poolJoiningTitle", detail: "match.poolJoiningDetail" },
  creating: { title: "match.poolCreatingTitle", detail: "match.poolCreatingDetail" },
  in_room: { title: "match.poolInRoomTitle", detail: "match.poolInRoomDetail" },
};

const ROOM_COPY: Readonly<Record<LocalRoomRole, StatusCopy>> = {
  joining: { title: "match.roomJoiningTitle", detail: "match.roomJoiningDetail" },
  seated: { title: "match.roomSeatedTitle", detail: "match.roomSeatedDetail" },
  waiting: { title: "match.roomWaitingTitle", detail: "match.roomWaitingDetail" },
  playing: { title: "match.roomPlayingTitle", detail: "match.roomPlayingDetail" },
  leaving: { title: "match.roomLeavingTitle", detail: "match.roomLeavingDetail" },
};

const SEAT_POSITIONS: Readonly<Record<number, CSSProperties>> = {
  1: { left: "50%", top: "2%", transform: "translate(-50%, -50%)" },
  2: { left: "93%", top: "28%", transform: "translate(-50%, -50%)" },
  3: { left: "87%", top: "78%", transform: "translate(-50%, -50%)" },
  4: { left: "50%", top: "98%", transform: "translate(-50%, -50%)" },
  5: { left: "13%", top: "78%", transform: "translate(-50%, -50%)" },
  6: { left: "7%", top: "28%", transform: "translate(-50%, -50%)" },
};

const AVATAR_TONES = ["#173e82", "#0d7969", "#302b72", "#ca5e41", "#0f2e35", "#765321"] as const;

function shortenedPlayerId(playerId: string): string {
  return playerId.length <= 12 ? playerId : `${playerId.slice(0, 6)}…${playerId.slice(-4)}`;
}

function PlayerAvatar({
  avatarUrl,
  seat,
}: {
  readonly avatarUrl: string | null;
  readonly seat: number;
}) {
  if (avatarUrl !== null) {
    return <img src={avatarUrl} alt="" className="size-9 rounded-full object-cover ring-1 ring-black/10" />;
  }
  return (
    <span
      className="grid size-9 shrink-0 place-items-center rounded-full text-white ring-1 ring-black/10"
      style={{ backgroundColor: AVATAR_TONES[(seat - 1) % AVATAR_TONES.length] }}
      aria-hidden="true"
    >
      <CodexMark className="size-[18px]" />
    </span>
  );
}

function SearchSurface({ bridge }: { readonly bridge: BridgeSnapshot }) {
  const { t } = useI18n();
  const status = POOL_COPY[bridge.pool.status];
  const level = STAKE_LEVELS.find((candidate) => candidate.id === bridge.pool.levelId);

  return (
    <div className="relative m-auto flex w-full max-w-[760px] flex-col items-center text-center">
      <div className="pointer-events-none absolute left-1/2 top-[42%] -z-10 size-[min(68vw,720px)] -translate-x-1/2 -translate-y-1/2 rounded-full border border-black/[.035]" />
      <div className="pointer-events-none absolute left-1/2 top-[42%] -z-10 size-[min(46vw,500px)] -translate-x-1/2 -translate-y-1/2 rounded-full border border-black/[.045]" />
      <motion.div
        className="relative grid size-16 place-items-center rounded-[20px] border border-black/[.075] bg-white shadow-[0_14px_44px_rgba(22,28,24,.08)]"
        animate={{ scale: [1, 1.035, 1] }}
        transition={{ duration: 2.4, repeat: Number.POSITIVE_INFINITY, ease: "easeInOut" }}
      >
        <Radio className="size-6 text-[var(--codex-blue-500)]" strokeWidth={1.65} />
        <span className="absolute -right-1 -top-1 size-3 rounded-full border-2 border-white bg-[var(--codex-blue-500)] shadow-[0_0_0_5px_var(--codex-blue-50)]" />
      </motion.div>
      <h1 className="mt-7 text-[clamp(24px,3vw,38px)] font-semibold tracking-[-0.045em]">
        {t(status.title)}
      </h1>
      <p className="mt-3 text-[12px] leading-5 text-[var(--muted)]">{t(status.detail)}</p>

      <dl className="mt-8 grid w-full grid-cols-3 divide-x divide-[var(--line)] rounded-[14px] border border-[var(--line)] bg-white py-4 shadow-[var(--codex-shadow-xs)]">
        <div>
          <dt className="text-[9px] text-[var(--muted-light)]">{t("match.level")}</dt>
          <dd className="mt-1 text-[11px] font-medium">{level ? t(level.nameKey) : bridge.pool.levelId ?? t("match.pending")}</dd>
        </div>
        <div>
          <dt className="text-[9px] text-[var(--muted-light)]">{t("match.availableTables")}</dt>
          <dd className="mt-1 text-[11px] font-medium tabular-nums">{bridge.pool.discoveredTables}</dd>
        </div>
        <div>
          <dt className="text-[9px] text-[var(--muted-light)]">{t("match.poolPlayers")}</dt>
          <dd className="mt-1 text-[11px] font-medium tabular-nums">{bridge.pool.waitingPlayers}</dd>
        </div>
      </dl>
      <p className="mt-5 inline-flex items-center gap-2 text-[10px] text-[var(--muted-light)]">
        <ShieldCheck className="size-3.5" strokeWidth={1.7} />
        {t("match.fullTableRule")}
      </p>
    </div>
  );
}

function RoomSurface({ bridge }: { readonly bridge: BridgeSnapshot }) {
  const { t, formatTokens } = useI18n();
  const room = bridge.room;
  const profile = projectPlayerProfile(
    bridge.tokenSnapshot,
    t(bridge.mode === "preview" ? "app.previewPlayer" : "app.codexPlayer"),
  );
  const copy = room.localRole === null ? POOL_COPY.in_room : ROOM_COPY[room.localRole];
  const localPlayerId = bridge.identity?.playerId ?? null;

  return (
    <div className="m-auto flex w-full max-w-[1120px] flex-col items-center">
      <div className="text-center">
        <h1 className="text-[clamp(22px,2.5vw,34px)] font-semibold tracking-[-0.045em]">{t(copy.title)}</h1>
        <p className="mt-2 text-[11px] text-[var(--muted)]">{t(copy.detail)}</p>
      </div>

      <div className="relative mt-10 aspect-[2.15/1] w-[min(82vw,960px)]">
        <div className="absolute inset-[7%_5%] rounded-[50%] border border-black/[.085] bg-[#f5f6f5] shadow-[inset_0_1px_0_white,0_22px_60px_rgba(22,28,24,.06)]">
          <div className="absolute inset-[10%] rounded-[50%] border border-black/[.035]" />
          <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 text-center">
            <div className="mx-auto grid size-10 place-items-center rounded-[12px] border border-black/[.07] bg-white shadow-sm">
              <CodexMark className="size-5 text-[var(--ink)]" />
            </div>
            <p className="mt-3 text-[12px] font-semibold tracking-[-.02em]">{t("match.nextHand")}</p>
            <p className="mt-1 text-[9px] text-[var(--muted-light)]">
              {t("match.seatSummary", { seated: room.seats.length, waiting: room.waiting.length })}
            </p>
          </div>
        </div>

        {Array.from({ length: room.capacity }, (_, index) => index + 1).map((physicalSeat) => {
          const seat = room.seats.find((candidate) => candidate.physicalSeat === physicalSeat);
          const isLocal = seat?.playerId === localPlayerId;
          return (
            <div key={physicalSeat} className="absolute z-10" style={SEAT_POSITIONS[physicalSeat]}>
              {seat === undefined ? (
                <div className="grid h-[62px] w-[154px] place-items-center rounded-[18px] border border-dashed border-black/[.09] bg-white/70 text-[10px] text-[var(--muted-light)] backdrop-blur-sm">
                  {t("match.emptySeat")}
                </div>
              ) : (
                <motion.div
                  initial={{ opacity: 0, scale: 0.92 }}
                  animate={{ opacity: 1, scale: 1 }}
                  transition={{ type: "spring", stiffness: 360, damping: 28 }}
                  className={`flex h-[64px] w-[176px] items-center gap-3 rounded-[20px] border bg-white px-3 shadow-[0_10px_30px_rgba(22,28,24,.08)] ${
                    isLocal ? "border-[#75bdef] ring-2 ring-[#d9effd]" : "border-black/[.09]"
                  }`}
                >
                  <PlayerAvatar avatarUrl={isLocal ? profile.avatarUrl : null} seat={physicalSeat} />
                  <span className="min-w-0 text-left">
                    <span className="block truncate text-[11px] font-semibold">
                      {isLocal ? profile.displayName : shortenedPlayerId(seat.playerId)}
                    </span>
                    <span className="mt-0.5 block text-[9px] text-[var(--muted-light)]">
                      {formatTokens(seat.buyIn)} Token{isLocal ? t("match.localSuffix") : ""}
                    </span>
                  </span>
                </motion.div>
              )}
            </div>
          );
        })}
      </div>

      <div className="mt-6 flex items-center gap-3 text-[10px] text-[var(--muted)]">
        {room.nextHandCountdownMs === null ? (
          <><Users className="size-3.5" />{t("match.joinLeaveAnytime")}</>
        ) : (
          <><Clock3 className="size-3.5" />{t("match.nextHandCountdown", { seconds: Math.ceil(room.nextHandCountdownMs / 1_000) })}</>
        )}
        {room.membershipRequired > 0 ? (
          <span className="inline-flex items-center gap-1.5 border-l border-[var(--line)] pl-3">
            <CircleCheck className="size-3.5 text-[var(--codex-green-500)]" />
            {t("match.memberConfirmation", { confirmed: room.membershipConfirmed, required: room.membershipRequired })}
          </span>
        ) : null}
      </div>
    </div>
  );
}

export function MatchmakingView({ bridge, sendCommand, sendConfirmedCommand }: MatchmakingViewProps) {
  const { t } = useI18n();
  const safeLeave = useSafeLeave(sendConfirmedCommand, t("bridge.commandFailed"));
  const hasRoom = bridge.room.tableId !== null;
  const isFriendRoom = bridge.friendRoomStatus !== "idle";
  const shouldLeaveTable = hasRoom || isFriendRoom;
  const isLeaving = safeLeave.isPending || bridge.room.localRole === "leaving";
  const leave = (): void => {
    if (isLeaving) return;
    if (shouldLeaveTable) {
      void safeLeave.request();
      return;
    }
    sendCommand({ type: "cancel_public_pool" });
  };

  return (
    <section className="relative flex h-full min-h-0 w-full overflow-hidden bg-white">
      <div className="relative z-10 flex min-h-0 w-full flex-col px-[clamp(24px,5vw,72px)] py-[clamp(24px,5vh,52px)]">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <StatusPill label={t(isFriendRoom ? "match.privateRoom" : hasRoom ? "match.dynamicTable" : "match.autoFind")} icon={isFriendRoom ? Users : Radio} />
            <StatusPill
              label={t(bridge.discovery.registeredNodes.size > 0 ? "match.communityConnected" : "match.communityConnecting")}
              tone={bridge.discovery.registeredNodes.size > 0 ? "success" : "attention"}
              dot
            />
          </div>
          <Button variant="secondary" size="sm" disabled={isLeaving} onClick={leave}>
            {shouldLeaveTable ? <LogOut className="size-3.5" /> : <X className="size-3.5" />}
            {t(isLeaving ? "table.leavingShort" : shouldLeaveTable ? "match.safeLeave" : "match.stopSearch")}
          </Button>
        </div>

        {bridge.friendInviteCode !== null ? (
          <div className="absolute left-1/2 top-[clamp(24px,5vh,52px)] -translate-x-1/2 rounded-[11px] border border-[var(--line)] bg-white px-4 py-2.5 text-center shadow-[var(--codex-shadow-xs)]">
            <p className="text-[8px] font-medium uppercase tracking-[.12em] text-[var(--muted-light)]">{t("match.inviteCode")}</p>
            <p className="mt-0.5 font-mono text-[11px] font-semibold tracking-[.06em]">{bridge.friendInviteCode}</p>
          </div>
        ) : null}

        {safeLeave.state.status === "failed" ? (
          <p className="absolute left-1/2 top-[clamp(66px,10vh,96px)] -translate-x-1/2 text-[10px] text-[var(--codex-red-600)]" role="alert">
            {safeLeave.state.error}
          </p>
        ) : null}

        {hasRoom ? <RoomSurface bridge={bridge} /> : <SearchSurface bridge={bridge} />}
      </div>
    </section>
  );
}
