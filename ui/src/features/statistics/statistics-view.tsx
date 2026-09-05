import {
  ArrowDownRight,
  ArrowUpRight,
  CloudOff,
  FileCheck2,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import type {
  BridgeSnapshot,
  CommandResult,
  HostCommand,
  RecentHandSnapshot,
} from "../../core/bridge/contracts";
import { useI18n } from "../../core/i18n/use-i18n";
import type { MessageKey, MessageVariables } from "../../core/i18n/messages";
import { Button } from "../../shared/ui/button";
import { SectionHeader } from "../../shared/ui/section-header";
import { StatusPill } from "../../shared/ui/status-pill";

interface StatisticsViewProps {
  readonly bridge: BridgeSnapshot;
  readonly sendCommand: (command: HostCommand) => CommandResult;
}

export function StatisticsView({ bridge, sendCommand }: StatisticsViewProps) {
  const { t, formatDate, formatInteger, formatSignedTokens, formatTokens } = useI18n();
  const metrics = bridge.statistics;
  const chart = cumulativeChart(metrics.recentHands);
  const archiveReady = bridge.archive.peers.length > 0;

  return (
    <section className="h-full overflow-y-auto bg-[var(--canvas)]">
      <div className="page-content">
        <SectionHeader
          eyebrow={t("stats.eyebrow")}
          title={t("stats.title")}
          description={t("stats.description")}
          action={
            <div className="flex items-center gap-2">
              <StatusPill
                icon={archiveReady ? ShieldCheck : CloudOff}
                label={archiveReady ? t("stats.archiveNodes", { count: bridge.archive.peers.length }) : t("stats.noArchive")}
                tone={archiveReady ? "success" : "attention"}
              />
              <Button
                variant="secondary"
                size="sm"
                disabled={bridge.identity === null || !archiveReady}
                onClick={() => sendCommand({ type: "sync_statistics" })}
              >
                <RefreshCw className="size-3.5" />
                {t("common.sync")}
              </Button>
            </div>
          }
        />

        <div className="mt-6 grid grid-cols-2 gap-px overflow-hidden rounded-[15px] border border-[var(--line)] bg-[var(--line)] min-[760px]:grid-cols-4">
          {[
            [t("stats.completedHands"), formatInteger(metrics.completedHands)],
            [t("stats.net"), `${formatSignedTokens(metrics.netChips)} Token`],
            [t("stats.grossWon"), `${formatTokens(metrics.grossWon)} Token`],
            [t("stats.grossLost"), `${formatTokens(metrics.grossLost)} Token`],
          ].map(([label, value]) => (
            <div key={label} className="min-w-0 bg-white px-5 py-4">
              <p className="text-xs text-[var(--muted)]">{label}</p>
              <p className="mt-2 text-[18px] font-semibold tabular-nums tracking-[-0.035em]">{value}</p>
            </div>
          ))}
        </div>

        <div className="mt-5 grid grid-cols-1 gap-5 min-[900px]:grid-cols-[minmax(0,1.5fr)_minmax(250px,.5fr)]">
          <div className="rounded-[15px] border border-[var(--line)] bg-white p-5">
            <div className="flex items-start justify-between">
              <div>
                <p className="text-[12px] font-semibold">{t("stats.cumulative")}</p>
                <p className="mt-1 text-xs text-[var(--muted)]">
                  {t("stats.recentVerified", { count: metrics.recentHands.length })}
                </p>
              </div>
              <div className="text-right">
                <p className={`text-[16px] font-semibold tabular-nums ${metrics.netChips >= 0 ? "text-[#2f7b43]" : "text-[#a64640]"}`}>
                  {formatSignedTokens(metrics.netChips)}
                </p>
                <p className="mt-1 text-xs text-[var(--muted)]">{t("stats.displayOnly")}</p>
              </div>
            </div>
            <div className="relative mt-7 flex h-[154px] items-end gap-2 border-b border-black/[.07] px-2">
              <div className="absolute inset-x-0 top-1/2 border-t border-dashed border-black/[.06]" />
              {chart.length > 0 ? (
                chart.map((point) => (
                  <div key={point.key} className="group relative flex h-full flex-1 items-end">
                    <div
                      className={`w-full rounded-t-[4px] transition-colors ${point.positive ? "bg-[#dcece0] group-hover:bg-[#8fc69c]" : "bg-[#f0ddda] group-hover:bg-[#d89a92]"}`}
                      style={{ height: `${String(point.height)}%` }}
                      title={formatSignedTokens(point.value)}
                    />
                  </div>
                ))
              ) : (
                <div className="mb-12 w-full text-center text-xs text-[var(--muted)]">
                  {t("stats.emptyChart")}
                </div>
              )}
            </div>
            <div className="mt-2 flex justify-between text-xs text-[var(--muted)]">
              <span>{t("stats.older")}</span>
              <span>{t("stats.current")}</span>
            </div>
          </div>

          <div className="rounded-[15px] border border-[var(--line)] bg-white p-5">
            <p className="text-[12px] font-semibold">{t("stats.composition")}</p>
            <div className="mt-5 space-y-4">
              {[
                [t("stats.won"), metrics.wonHands, "bg-[#69ad78]"],
                [t("stats.lost"), metrics.lostHands, "bg-[#cf756c]"],
                [t("stats.split"), metrics.splitHands, "bg-[#aeb7b0]"],
              ].map(([label, count, tone]) => {
                const numericCount = Number(count);
                const ratio = metrics.completedHands === 0 ? 0 : (numericCount / metrics.completedHands) * 100;
                return (
                  <div key={String(label)}>
                    <div className="flex items-center justify-between text-xs">
                      <span className="text-[var(--muted)]">{label}</span>
                      <span className="font-medium tabular-nums">{String(numericCount)}</span>
                    </div>
                    <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[#edf0ed]">
                      <div className={`h-full rounded-full ${String(tone)}`} style={{ width: `${String(ratio)}%` }} />
                    </div>
                  </div>
                );
              })}
            </div>
            <dl className="mt-6 border-t border-[var(--line)] pt-4 text-xs">
              <div className="flex justify-between">
                <dt className="text-[var(--muted)]">{t("stats.largestWin")}</dt>
                <dd className="font-medium tabular-nums">{formatTokens(metrics.largestWin)}</dd>
              </div>
              <div className="mt-2.5 flex justify-between">
                <dt className="text-[var(--muted)]">{t("stats.largestLoss")}</dt>
                <dd className="font-medium tabular-nums">{formatTokens(metrics.largestLoss)}</dd>
              </div>
            </dl>
          </div>
        </div>

        <div className="mt-5 overflow-hidden rounded-[15px] border border-[var(--line)] bg-white">
          <div className="flex items-center justify-between border-b border-[var(--line)] px-5 py-4">
            <div>
              <h2 className="text-[12px] font-semibold">{t("stats.recentHands")}</h2>
              <p className="mt-1 text-xs text-[var(--muted)]">{t("stats.recentDescription")}</p>
            </div>
            <FileCheck2 className="size-4 text-[var(--muted)]" strokeWidth={1.7} />
          </div>
          {metrics.recentHands.length > 0 ? (
            <div className="overflow-x-auto outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)]" role="region" aria-label={t("stats.recentHands")} tabIndex={0}>
              <table className="w-full min-w-[640px] border-collapse text-left">
                <thead>
                  <tr className="border-b border-[var(--line)] text-xs font-medium text-[var(--muted)]">
                    <th className="px-5 py-2.5">{t("stats.hand")}</th>
                    <th className="px-3 py-2.5">{t("stats.level")}</th>
                    <th className="px-3 py-2.5">{t("stats.players")}</th>
                    <th className="px-3 py-2.5">{t("stats.result")}</th>
                    <th className="px-3 py-2.5">{t("stats.receipt")}</th>
                    <th className="px-5 py-2.5 text-right">{t("stats.time")}</th>
                  </tr>
                </thead>
                <tbody>
                  {metrics.recentHands.map((hand) => {
                    const positive = hand.delta >= 0;
                    const ResultIcon = positive ? ArrowUpRight : ArrowDownRight;
                    return (
                      <tr key={hand.address} className="border-b border-black/[.045] last:border-b-0 hover:bg-black/[.018]">
                        <td className="px-5 py-3 text-[13px] font-medium" title={hand.receiptId}>
                          #{String(hand.handNumber)}
                        </td>
                        <td className="px-3 py-3 text-xs text-[var(--muted)]">{formatLevel(hand.levelId)}</td>
                        <td className="px-3 py-3 text-xs text-[var(--muted)]">{t("stats.playerCount", { count: hand.players })}</td>
                        <td className={`px-3 py-3 text-[13px] font-semibold tabular-nums ${positive ? "text-[#327844]" : "text-[#b4443e]"}`}>
                          <span className="inline-flex items-center gap-1">
                            <ResultIcon className="size-3" /> {formatSignedTokens(hand.delta)}
                          </span>
                        </td>
                        <td className={`px-3 py-3 text-xs ${hand.archived ? "text-[#4b7652]" : "text-[#9a6c22]"}`}>
                          {t(hand.archived ? "stats.archived" : "stats.notArchived")}
                        </td>
                        <td className="px-5 py-3 text-right text-xs text-[var(--muted)]">
                          {relativeTime(hand.settledAtUnixMs, t, formatDate)}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          ) : (
            <div className="px-5 py-14 text-center">
              <p className="text-[12px] font-medium">{t("stats.emptyTitle")}</p>
              <p className="mt-2 text-xs text-[var(--muted)]">{t("stats.emptyDescription")}</p>
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

function cumulativeChart(hands: readonly RecentHandSnapshot[]) {
  let cumulative = 0;
  const points = [...hands]
    .reverse()
    .map((hand) => {
      cumulative += hand.delta;
      return { key: hand.address, value: cumulative };
    });
  const maximum = Math.max(1, ...points.map((point) => Math.abs(point.value)));
  return points.map((point) => ({
    ...point,
    positive: point.value >= 0,
    height: Math.max(4, 12 + (Math.abs(point.value) / maximum) * 78),
  }));
}

function formatLevel(levelId: string): string {
  return levelId.replaceAll("k", "K").replaceAll("m", "M").replace("-", "/");
}

function relativeTime(
  timestamp: number,
  t: (key: MessageKey, variables?: MessageVariables) => string,
  formatDate: (value: Date | number) => string,
): string {
  const elapsed = Math.max(0, Date.now() - timestamp);
  if (elapsed < 60_000) return t("stats.justNow");
  if (elapsed < 3_600_000) return t("stats.minutesAgo", { count: Math.floor(elapsed / 60_000) });
  if (elapsed < 86_400_000) return t("stats.hoursAgo", { count: Math.floor(elapsed / 3_600_000) });
  return formatDate(timestamp);
}
