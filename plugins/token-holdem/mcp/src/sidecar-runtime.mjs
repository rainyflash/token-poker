import { spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { createConnection } from "node:net";
import { createInterface } from "node:readline";
import {
  isCacheableVerifiedNode,
  recordVerifiedNode,
} from "../../../../scripts/community-network.mjs";
import {
  createRuntimeLaunchPlan,
  saveRuntimeVolunteerConsent,
} from "./runtime-launch-plan.mjs";
import {
  parseRuntimeFrame,
  runtimeAttachFrame,
  RUNTIME_PROTOCOL_VERSION,
} from "./runtime-protocol.mjs";
import { SessionEventProjection } from "./session-event-projection.mjs";

const EVENT_BUFFER_LIMIT = 4_096;
const CONNECT_TIMEOUT_MS = 15_000;
const CONNECT_RETRY_MS = 100;
const TOKEN_ACK_TIMEOUT_MS = 10_000;
const IDENTITY_ACK_TIMEOUT_MS = 10_000;
const COMMAND_ACK_TIMEOUT_MS = 10_000;
export const MAX_POLL_WAIT_MS = 25_000;

export class SidecarRuntime {
  #pluginRoot;
  #socket = null;
  #lines = null;
  #starting = null;
  #closing = false;
  #attached = false;
  #attachResolve = null;
  #attachReject = null;
  #runtimeId = null;
  #runtimeGeneration = 0;
  #runtimeSequence = 0;
  #poolActive = false;
  #roomActive = false;
  #handActive = false;
  #events = [];
  #sessionProjection = new SessionEventProjection();
  #sequence = 0;
  #eventSignal = new EventEmitter();
  #launchPlan = null;
  #tokenSnapshot = null;
  #accountBinding = null;
  #identitySnapshot = null;
  #serviceCacheQueue = Promise.resolve();

  constructor(pluginRoot) {
    this.#pluginRoot = pluginRoot;
  }

  get latestSequence() {
    return this.#sequence;
  }

  get ready() {
    return this.#attached && this.#socket !== null && !this.#socket.destroyed;
  }

  get tokenSnapshot() {
    return this.#tokenSnapshot;
  }

  get accountBinding() {
    return this.#accountBinding;
  }

  get currentState() {
    return Object.freeze({
      identity: this.#identitySnapshot,
      latest_sequence: this.#sequence,
      events: this.#sessionProjection.snapshot(),
    });
  }

  async ensureStarted() {
    if (this.ready) return;
    if (this.#closing) throw new Error("Token Poker MCP 连接正在退出");
    if (this.#starting !== null) return this.#starting;
    this.#starting = this.#start().finally(() => {
      this.#starting = null;
    });
    return this.#starting;
  }

  async send(command) {
    await this.ensureStarted();
    await this.#write(command);
  }

  async ensureIdentity(command, requestId) {
    const event = await this.#sendConfirmedCommand(
      command,
      requestId,
      IDENTITY_ACK_TIMEOUT_MS,
      (candidate) => candidate.type === "identity_ready",
      "牌局内核未在超时前确认玩家身份",
    );
    const identity = parseIdentitySnapshot(event);
    if (identity === null) throw new Error("牌局内核返回了无效的玩家身份确认");
    return identity;
  }

  async leaveTable(command, requestId) {
    await this.#sendConfirmedCommand(
      command,
      requestId,
      COMMAND_ACK_TIMEOUT_MS,
      (candidate) =>
        candidate.type === "command_confirmed" && candidate.command_type === "leave_table",
      "牌局内核未在超时前确认离桌请求",
    );
  }

  async publishTokenSnapshot(command) {
    const initialSequence = this.#sequence;
    await this.send(command);
    let cursor = initialSequence;
    const deadline = Date.now() + TOKEN_ACK_TIMEOUT_MS;
    while (Date.now() < deadline) {
      const batch = await this.waitForEvents(
        cursor,
        Math.min(1_000, Math.max(0, deadline - Date.now())),
      );
      const accepted = batch.events.find(
        (entry) =>
          entry.event?.type === "token_snapshot_accepted" &&
          entry.event.observed_at_unix_ms === command.observed_at_unix_ms &&
          entry.event.lifetime_tokens === command.lifetime_tokens,
      )?.event;
      if (accepted !== undefined) return accepted;
      cursor = batch.latest_sequence;
    }
    throw new Error("牌局内核未在超时前确认官方 Token 快照");
  }

  async #sendConfirmedCommand(command, requestId, timeoutMs, isConfirmation, timeoutMessage) {
    validateRequestId(requestId);
    const initialSequence = this.#sequence;
    await this.send({ ...command, request_id: requestId });
    let cursor = initialSequence;
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const batch = await this.waitForEvents(
        cursor,
        Math.min(1_000, Math.max(0, deadline - Date.now())),
      );
      for (const entry of batch.events) {
        const event = entry.event;
        if (event?.request_id !== requestId) continue;
        if (event.type === "command_failed") {
          throw new Error(
            typeof event.message === "string" ? event.message : "牌局内核拒绝了控制命令",
          );
        }
        if (isConfirmation(event)) return event;
      }
      cursor = batch.latest_sequence;
    }
    throw new Error(timeoutMessage);
  }

  async setVolunteerConsent(enabled) {
    const consent = await saveRuntimeVolunteerConsent(enabled);
    const restarted = this.#isBusy() ? false : await this.#restart();
    this.#publish({
      type: "volunteer_preference_saved",
      consent,
      restart_required: !restarted,
    });
  }

  readEvents(afterSequence) {
    validateSequence(afterSequence);
    const firstAvailableSequence = this.#events[0]?.sequence ?? this.#sequence + 1;
    return Object.freeze({
      events: this.#sessionProjection.merge(this.#events, afterSequence),
      latest_sequence: this.#sequence,
      history_truncated: afterSequence + 1 < firstAvailableSequence,
      sidecar_ready: this.ready,
      runtime_id: this.#runtimeId,
      runtime_generation: this.#runtimeGeneration,
      current_state: this.currentState,
    });
  }

  async waitForEvents(afterSequence, waitMs) {
    validateSequence(afterSequence);
    if (!Number.isSafeInteger(waitMs) || waitMs < 0 || waitMs > MAX_POLL_WAIT_MS) {
      throw new Error(`轮询等待时间必须在 0 到 ${String(MAX_POLL_WAIT_MS)} 毫秒之间`);
    }
    await this.ensureStarted();
    if (this.#sequence > afterSequence || waitMs === 0) {
      return this.readEvents(afterSequence);
    }
    await new Promise((resolveWait) => {
      const onEvent = () => {
        clearTimeout(timeout);
        resolveWait();
      };
      const timeout = setTimeout(() => {
        this.#eventSignal.removeListener("event", onEvent);
        resolveWait();
      }, waitMs);
      this.#eventSignal.once("event", onEvent);
    });
    return this.readEvents(afterSequence);
  }

  async close() {
    this.#closing = true;
    this.#rejectAttach(new Error("Token Poker MCP 连接正在退出"));
    const socket = this.#socket;
    this.#detachSocket();
    if (socket !== null && !socket.destroyed) {
      socket.end();
      socket.destroy();
    }
    if (this.#starting !== null) await this.#starting.catch(() => undefined);
    await this.#serviceCacheQueue;
  }

  async terminateRuntimeForTesting() {
    await this.ensureStarted();
    const socket = this.#socket;
    if (socket === null) return;
    const closed = new Promise((resolveClose) => socket.once("close", resolveClose));
    await this.#write({ type: "runtime_shutdown" });
    await Promise.race([
      closed,
      new Promise((resolveTimeout) => setTimeout(resolveTimeout, 5_000)),
    ]);
    await this.close();
  }

  async #start() {
    const plan = await createRuntimeLaunchPlan(this.#pluginRoot);
    this.#launchPlan = plan;
    this.#publishLaunchPlan(plan);
    let socket;
    try {
      socket = await connectToRuntime(plan.pipeName);
    } catch (error) {
      if (error?.code === "EBUSY") {
        socket = await waitForRuntime(plan.pipeName, CONNECT_TIMEOUT_MS);
      } else {
        if (!isRuntimeMissing(error)) throw error;
        startSupervisor(plan);
        socket = await waitForRuntime(plan.pipeName, CONNECT_TIMEOUT_MS);
      }
    }
    await this.#attachSocket(socket);
  }

  #publishLaunchPlan(plan) {
    if (plan.hostWarning !== null) {
      this.#publish({
        type: "warning",
        message: `Windows 网络与电源状态探测失败，志愿 Relay 将保守关闭：${plan.hostWarning}`,
      });
    }
    this.#publish({
      type: "community_network_loaded",
      rendezvous_nodes: plan.networkPlan.rendezvous.length,
      relay_nodes: plan.networkPlan.relays.length,
      archive_nodes: plan.networkPlan.archives.length,
      cold_start_available: plan.networkPlan.rendezvous.length > 0,
    });
  }

  async #attachSocket(socket) {
    this.#socket = socket;
    this.#attached = false;
    const lines = createInterface({ input: socket, crlfDelay: Infinity });
    this.#lines = lines;
    const replayComplete = new Promise((resolveReplay, rejectReplay) => {
      this.#attachResolve = resolveReplay;
      this.#attachReject = rejectReplay;
    });
    lines.on("line", (line) => this.#handleRuntimeLine(socket, line));
    socket.on("error", (error) => {
      if (!this.#closing) {
        this.#publish({ type: "warning", message: `共享牌局运行时连接失败：${error.message}` });
      }
    });
    socket.on("close", () => this.#handleSocketClose(socket));
    try {
      await this.#write(runtimeAttachFrame());
      await withTimeout(replayComplete, CONNECT_TIMEOUT_MS, "共享运行时状态回放超时");
    } catch (error) {
      if (this.#socket === socket) this.#detachSocket();
      socket.destroy();
      throw error;
    }
  }

  #handleRuntimeLine(socket, line) {
    if (this.#socket !== socket) return;
    let frame;
    try {
      frame = parseRuntimeFrame(line);
    } catch (error) {
      this.#rejectAttach(error);
      this.#publish({
        type: "warning",
        message: error instanceof Error ? error.message : "共享运行时帧无效",
      });
      return;
    }
    switch (frame.type) {
      case "runtime_attached":
        this.#acceptAttachedFrame(frame);
        break;
      case "runtime_event":
        this.#acceptRuntimeEvent(frame);
        break;
      case "runtime_replay_complete":
        if (frame.generation !== this.#runtimeGeneration) {
          this.#rejectAttach(new Error("共享运行时回放代次不一致"));
          break;
        }
        this.#attached = true;
        this.#attachResolve?.();
        this.#clearAttachCallbacks();
        break;
      case "runtime_error": {
        const error = new Error(`共享运行时拒绝请求（${frame.code}）：${frame.message}`);
        if (!this.#attached) this.#rejectAttach(error);
        this.#publish({ type: "warning", message: error.message });
        break;
      }
      default:
        break;
    }
  }

  #acceptAttachedFrame(frame) {
    if (frame.protocol_version !== RUNTIME_PROTOCOL_VERSION) {
      this.#rejectAttach(new Error("共享运行时协议版本不兼容"));
      return;
    }
    const runtimeChanged = this.#runtimeId !== null && this.#runtimeId !== frame.runtime_id;
    const generationChanged =
      this.#runtimeGeneration !== 0 && this.#runtimeGeneration !== frame.generation;
    if (runtimeChanged || generationChanged) this.#resetProjection();
    this.#runtimeId = frame.runtime_id;
    this.#runtimeGeneration = frame.generation;
    if (frame.history_truncated) {
      this.#publish({
        type: "warning",
        message: "共享运行时的旧诊断事件已截断；当前身份、匹配、房间与手牌状态已从保留投影恢复。",
      });
    }
  }

  #acceptRuntimeEvent(frame) {
    if (frame.generation !== this.#runtimeGeneration) {
      if (frame.generation < this.#runtimeGeneration) return;
      this.#resetProjection();
      this.#runtimeGeneration = frame.generation;
    }
    if (frame.sequence <= this.#runtimeSequence) return;
    this.#runtimeSequence = frame.sequence;
    this.#consumeSidecarEvent(frame.event, this.#attached);
  }

  #consumeSidecarEvent(event, shouldCacheService) {
    this.#updateBusyState(event.type);
    if (event.type === "identity_ready") {
      const identity = parseIdentitySnapshot(event);
      if (identity !== null) this.#identitySnapshot = identity;
    }
    if (event.type === "token_snapshot_accepted") {
      const {
        lifetime_tokens: lifetimeTokens,
        observed_at_unix_ms: observedAtUnixMs,
        account_fingerprint: accountFingerprint,
        peer_verifiable: peerVerifiable,
      } = event;
      if (
        Number.isSafeInteger(lifetimeTokens) &&
        Number.isSafeInteger(observedAtUnixMs) &&
        typeof accountFingerprint === "string" &&
        accountFingerprint.length > 0 &&
        typeof peerVerifiable === "boolean"
      ) {
        this.#tokenSnapshot = Object.freeze({
          lifetime_tokens: lifetimeTokens,
          username: typeof event.username === "string" ? event.username : null,
          display_name: typeof event.display_name === "string" ? event.display_name : null,
          observed_at_unix_ms: observedAtUnixMs,
          source:
            event.source === "codex_app_server_account_usage"
              ? "codex_app_server_account_usage"
              : "shared_runtime_replay",
          observed_text: String(lifetimeTokens),
        });
        this.#accountBinding = Object.freeze({
          account_fingerprint: accountFingerprint,
          peer_verifiable: peerVerifiable,
        });
      }
    }
    this.#publish(event);
    if (shouldCacheService) {
      this.#serviceCacheQueue = this.#serviceCacheQueue
        .then(() => this.#cacheVerifiedService(event))
        .catch((error) => {
          process.stderr.write(`[token-holdem-mcp] 无法缓存社区节点：${error.message}\n`);
        });
    }
  }

  #handleSocketClose(socket) {
    if (this.#socket !== socket) return;
    const wasAttached = this.#attached;
    if (!this.#closing) {
      this.#rejectAttach(new Error("共享牌局运行时在附着完成前断开"));
    }
    this.#detachSocket();
    if (!this.#closing) {
      if (wasAttached) {
        this.#publish({
          type: "warning",
          message: "共享牌局运行时连接已断开；下一次轮询会自动重新附着。",
        });
      }
    }
  }

  #detachSocket() {
    this.#lines?.close();
    this.#lines = null;
    this.#socket = null;
    this.#attached = false;
    this.#clearAttachCallbacks();
    this.#eventSignal.emit("event");
  }

  #rejectAttach(error) {
    this.#attachReject?.(error);
    this.#clearAttachCallbacks();
  }

  #clearAttachCallbacks() {
    this.#attachResolve = null;
    this.#attachReject = null;
  }

  #resetProjection() {
    this.#runtimeSequence = 0;
    this.#events = [];
    this.#sessionProjection.clear();
    this.#poolActive = false;
    this.#roomActive = false;
    this.#handActive = false;
    this.#tokenSnapshot = null;
    this.#accountBinding = null;
    this.#identitySnapshot = null;
  }

  async #restart() {
    const plan = await createRuntimeLaunchPlan(this.#pluginRoot);
    this.#launchPlan = plan;
    await this.ensureStarted();
    await this.#write({
      type: "runtime_restart",
      worker_args: [...plan.workerArgs],
      bootstrap_commands: [...plan.bootstrapCommands],
    });
    return true;
  }

  async #write(frame) {
    const socket = this.#socket;
    if (socket === null || socket.destroyed || !socket.writable) {
      throw new Error("共享牌局运行时命令通道尚未就绪");
    }
    const encoded = `${JSON.stringify(frame)}\n`;
    await new Promise((resolveWrite, rejectWrite) => {
      socket.write(encoded, (error) => {
        if (error) rejectWrite(error);
        else resolveWrite();
      });
    });
  }

  #publish(event) {
    this.#sequence += 1;
    const entry = Object.freeze({ sequence: this.#sequence, event });
    this.#events.push(entry);
    this.#sessionProjection.observe(entry);
    if (this.#events.length > EVENT_BUFFER_LIMIT) {
      this.#events.splice(0, this.#events.length - EVENT_BUFFER_LIMIT);
    }
    this.#eventSignal.emit("event");
  }

  #updateBusyState(eventType) {
    if (eventType === "pool_joined") this.#poolActive = true;
    if (
      [
        "friend_room_created",
        "friend_room_joining",
        "friend_room_joined",
        "room_entered",
        "room_snapshot",
      ].includes(eventType)
    ) {
      this.#roomActive = true;
    }
    if (eventType === "pool_cancelled") this.#poolActive = false;
    if (eventType === "safe_leave_completed") {
      this.#poolActive = false;
      this.#roomActive = false;
      this.#handActive = false;
    }
    if (eventType === "room_closed") {
      this.#roomActive = false;
      this.#handActive = false;
    }
    if (["hand_protocol_started", "hand_ready", "hand_state"].includes(eventType)) {
      this.#handActive = true;
      this.#roomActive = true;
    }
    if (eventType === "hand_left" || eventType === "hand_aborted_for_leave") {
      this.#handActive = false;
    }
  }

  #isBusy() {
    return this.#poolActive || this.#roomActive || this.#handActive;
  }

  async #cacheVerifiedService(event) {
    const statePaths = this.#launchPlan?.statePaths;
    if (statePaths === undefined) return;
    const verified =
      event.type === "rendezvous_registered" &&
      typeof event.node === "string" &&
      typeof event.address === "string"
        ? { peerId: event.node, address: event.address, role: "rendezvous" }
        : event.type === "relay_reservation_accepted" &&
            typeof event.peer_id === "string" &&
            typeof event.address === "string"
          ? { peerId: event.peer_id, address: event.address, role: "relay" }
          : null;
    if (verified !== null && isCacheableVerifiedNode(verified)) {
      await recordVerifiedNode(statePaths.cache, verified);
    }
  }
}

function startSupervisor(plan) {
  const child = spawn(plan.runtimePath, plan.supervisorArgs, {
    detached: true,
    stdio: "ignore",
    windowsHide: true,
    env: {
      ...process.env,
      TOKEN_HOLDEM_BOOTSTRAP_COMMANDS: JSON.stringify(plan.bootstrapCommands),
    },
  });
  child.once("error", (error) => {
    process.stderr.write(`[token-holdem-mcp] 无法启动共享牌局运行时：${error.message}\n`);
  });
  child.unref();
}

async function waitForRuntime(pipeName, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  while (Date.now() < deadline) {
    try {
      return await connectToRuntime(pipeName);
    } catch (error) {
      lastError = error;
      if (!isRuntimeMissing(error) && error?.code !== "EBUSY") throw error;
      await delay(CONNECT_RETRY_MS);
    }
  }
  throw new Error(
    `共享牌局运行时启动超时${lastError instanceof Error ? `：${lastError.message}` : ""}`,
  );
}

function connectToRuntime(pipeName) {
  return new Promise((resolveConnect, rejectConnect) => {
    const socket = createConnection(pipeName);
    const onError = (error) => {
      socket.destroy();
      rejectConnect(error);
    };
    socket.once("error", onError);
    socket.once("connect", () => {
      socket.removeListener("error", onError);
      socket.setNoDelay(true);
      resolveConnect(socket);
    });
  });
}

function isRuntimeMissing(error) {
  return error?.code === "ENOENT" || error?.code === "ECONNREFUSED";
}

function withTimeout(promise, timeoutMs, message) {
  let timeoutHandle;
  const timeoutPromise = new Promise((_, rejectTimeout) => {
    timeoutHandle = setTimeout(() => rejectTimeout(new Error(message)), timeoutMs);
  });
  return Promise.race([promise, timeoutPromise]).finally(() => clearTimeout(timeoutHandle));
}

function delay(milliseconds) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

function validateSequence(value) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error("事件序号必须是非负安全整数");
  }
}

function validateRequestId(value) {
  if (
    typeof value !== "string" ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu.test(value)
  ) {
    throw new Error("命令请求 ID 必须是规范 UUID");
  }
}

function parseIdentitySnapshot(value) {
  if (
    value === null ||
    typeof value !== "object" ||
    typeof value.player_id !== "string" ||
    value.player_id.length === 0 ||
    typeof value.device_public_key !== "string" ||
    value.device_public_key.length === 0 ||
    typeof value.device_label !== "string" ||
    value.device_label.length === 0 ||
    !Number.isSafeInteger(value.certificate_expires_at_unix_ms) ||
    value.certificate_expires_at_unix_ms < 0 ||
    typeof value.recovery_envelope !== "string" ||
    value.recovery_envelope.length === 0 ||
    !Number.isSafeInteger(value.remote_replicas) ||
    value.remote_replicas < 0
  ) {
    return null;
  }
  return Object.freeze({
    player_id: value.player_id,
    device_public_key: value.device_public_key,
    device_label: value.device_label,
    certificate_expires_at_unix_ms: value.certificate_expires_at_unix_ms,
    recovery_envelope: value.recovery_envelope,
    remote_replicas: value.remote_replicas,
  });
}
