export interface CurrentStateProjection<TEvent> {
  readonly streamId: string;
  readonly latestSequence: number;
  readonly events: readonly TEvent[];
}

interface SequencedEvent<TEvent> {
  readonly sequence: number;
  readonly event: TEvent;
}

export function parseCurrentStateProjection<TEvent>(
  value: unknown,
  parseEvent: (value: unknown) => TEvent | null,
): CurrentStateProjection<TEvent> | null {
  if (!isRecord(value)) return null;
  const latestSequence = value.latest_sequence;
  if (!isNonNegativeSafeInteger(latestSequence) || !Array.isArray(value.events)) return null;

  const entries: SequencedEvent<TEvent>[] = [];
  for (const candidate of value.events) {
    if (!isRecord(candidate) || !isNonNegativeSafeInteger(candidate.sequence)) return null;
    if (candidate.sequence > latestSequence) return null;
    const event = parseEvent(candidate.event);
    if (event === null) return null;
    entries.push({ sequence: candidate.sequence, event });
  }
  entries.sort((left, right) => left.sequence - right.sequence);
  return {
    streamId: typeof value.stream_id === "string" ? value.stream_id : "legacy",
    latestSequence,
    events: entries.map((entry) => entry.event),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonNegativeSafeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) >= 0;
}
