const DEFAULT_FRESHNESS_MS = 5 * 60 * 1_000;

export class OfficialTokenService {
  #reader;
  #runtime;
  #now;
  #freshnessMs;
  #snapshot = null;
  #refreshing = null;

  constructor({ reader, runtime, now = Date.now, freshnessMs = DEFAULT_FRESHNESS_MS }) {
    if (typeof reader?.read !== "function") {
      throw new TypeError("官方 Token 服务需要账户用量读取端口");
    }
    if (typeof runtime?.publishTokenSnapshot !== "function") {
      throw new TypeError("官方 Token 服务需要牌局运行时端口");
    }
    if (!Number.isSafeInteger(freshnessMs) || freshnessMs < 0) {
      throw new RangeError("官方 Token 快照有效期无效");
    }
    this.#reader = reader;
    this.#runtime = runtime;
    this.#now = now;
    this.#freshnessMs = freshnessMs;
  }

  get snapshot() {
    return this.#snapshot ?? this.#runtime.tokenSnapshot;
  }

  async refresh({ force = false } = {}) {
    const current = this.snapshot;
    if (
      !force &&
      isFreshOfficialSnapshot(current, this.#now(), this.#freshnessMs) &&
      this.#runtime.accountBinding !== null
    ) {
      return current;
    }
    if (this.#refreshing !== null) return this.#refreshing;
    this.#refreshing = this.#readAndPublish().finally(() => {
      this.#refreshing = null;
    });
    return this.#refreshing;
  }

  async #readAndPublish() {
    const usage = await this.#reader.read();
    const snapshot = Object.freeze({
      lifetime_tokens: usage.lifetimeTokens,
      username: usage.username ?? null,
      display_name: usage.displayName ?? null,
      account_identifier: usage.accountIdentifier,
      observed_at_unix_ms: usage.observedAtUnixMs,
      observed_text: String(usage.lifetimeTokens),
      source: usage.source,
    });
    const accepted = await this.#runtime.publishTokenSnapshot({
      type: "token_snapshot",
      lifetime_tokens: snapshot.lifetime_tokens,
      username: snapshot.username,
      display_name: snapshot.display_name,
      account_identifier: snapshot.account_identifier,
      observed_at_unix_ms: snapshot.observed_at_unix_ms,
      source: snapshot.source,
    });
    const confirmedSnapshot = Object.freeze({
      ...snapshot,
      account_fingerprint: accepted.account_fingerprint,
      peer_verifiable: accepted.peer_verifiable,
    });
    this.#snapshot = confirmedSnapshot;
    return confirmedSnapshot;
  }
}

function isFreshOfficialSnapshot(snapshot, nowUnixMs, freshnessMs) {
  return (
    snapshot !== null &&
    snapshot.source === "codex_app_server_account_usage" &&
    Number.isSafeInteger(snapshot.observed_at_unix_ms) &&
    snapshot.observed_at_unix_ms <= nowUnixMs + 60_000 &&
    nowUnixMs - snapshot.observed_at_unix_ms <= freshnessMs
  );
}
