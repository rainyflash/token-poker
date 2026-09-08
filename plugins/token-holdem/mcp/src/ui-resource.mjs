import { readFile } from "node:fs/promises";
import { join } from "node:path";

export async function buildTokenHoldemHtml(pluginRoot, version) {
  const [vendorSource, styleSource, bundleSource] = await Promise.all([
    readFile(join(pluginRoot, "mcp", "vendor", "ext-apps-app-with-deps.js"), "utf8"),
    readFile(join(pluginRoot, "ui", "token-holdem.css"), "utf8"),
    readFile(join(pluginRoot, "ui", "token-holdem.js"), "utf8"),
  ]);
  const appClient = exposeBrowserExports(vendorSource, [
    "App",
    "applyDocumentTheme",
    "applyHostFonts",
    "applyHostStyleVariables",
  ]);
  const uiBundle = bundleSource.replace(/^\/\/# sourceMappingURL=.*$/gmu, "");
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta name="color-scheme" content="light" />
    <title>Token Poker</title>
    <style>
      html, body, #token-holdem-root { width: 100%; height: 100%; min-height: 100%; margin: 0; overflow: hidden; }
      #token-holdem-root { position: relative; }
      #token-poker-boot-status {
        position: absolute;
        inset: 0;
        display: grid;
        place-items: center;
        color: #6f6f6f;
        background: #fff;
        font: 14px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif;
      }
      #token-holdem-portals { position: relative; z-index: 1000; }
      ${styleSource}
    </style>
    <script>${escapeInlineScript(appClient)}</script>
  </head>
  <body>
    <div id="token-holdem-root"><div id="token-poker-boot-status" role="status" aria-live="polite">Opening Token Poker…</div></div>
    <div id="token-holdem-portals"></div>
    <script>${escapeInlineScript(buildHostBridge(version))}</script>
    <script>${escapeInlineScript(uiBundle)}</script>
  </body>
</html>`;
}

function exposeBrowserExports(source, exportNames) {
  const exportIndex = source.lastIndexOf("export{");
  if (exportIndex === -1) throw new Error("The MCP Apps browser bundle has no export block");
  const exportBlock = source.slice(exportIndex).match(/^export\{([^}]+)\};?\s*$/su);
  if (exportBlock === null) throw new Error("Could not parse the MCP Apps browser export block");
  const aliases = new Map();
  for (const rawEntry of exportBlock[1].split(",")) {
    const entry = rawEntry.trim();
    if (entry.length === 0) continue;
    const [localName, publicName = localName] = entry.split(/\s+as\s+/u).map((value) => value.trim());
    aliases.set(publicName, localName);
  }
  for (const name of exportNames) {
    if (!aliases.has(name)) throw new Error(`The MCP Apps browser bundle is missing export: ${name}`);
  }
  return [
    source.slice(0, exportIndex),
    ";globalThis.__TOKEN_HOLDEM_MCP_APPS__={",
    exportNames.map((name) => `${JSON.stringify(name)}:${aliases.get(name)}`).join(","),
    "};",
  ].join("");
}

export function buildHostBridge(version) {
  return `(() => {
  "use strict";

  const apps = globalThis.__TOKEN_HOLDEM_MCP_APPS__;
  const mountRoot = document.getElementById("token-holdem-root");
  const portalRoot = document.getElementById("token-holdem-portals");
  if (!apps || typeof apps.App !== "function" || !mountRoot || !portalRoot) {
    const message = "Token Poker could not initialize its Codex host bridge. Reopen the plugin or repair the installation.";
    globalThis.__tokenHoldemBootError = message;
    const bootStatus = document.getElementById("token-poker-boot-status");
    if (bootStatus) bootStatus.textContent = message;
    return;
  }

  globalThis.__tokenHoldemBridgeInstalled = true;
  globalThis.__tokenHoldemMountRoot = mountRoot;
  globalThis.__tokenHoldemPortalRoot = portalRoot;
  globalThis.__tokenPokerUpdateStatus = {
    phase: "idle",
    current_version: ${JSON.stringify(version)},
    latest_version: null,
    release_url: null,
    artifact_bytes: null,
    downloaded_bytes: 0,
    sha256_verified: false,
    error: null,
  };
  globalThis.__tokenHoldemBufferedSidecarEvents = Array.isArray(
    globalThis.__tokenHoldemBufferedSidecarEvents,
  )
    ? globalThis.__tokenHoldemBufferedSidecarEvents
    : [];

  let latestSequence = 0;
  let streamId = null;
  let latestPayloadSequence = -1;
  const retiredStreams = new Set();
  let eventsHydrated = false;
  let liveStateReceived = false;
  let bootstrapConsumed = false;
  let stopped = false;
  let resizeCleanup = null;
  let resizeSetupFailed = false;
  let displayModeSettled = false;
  let viewSuspended = document.visibilityState === "hidden";
  let resumeScheduled = false;
  let pollRefreshRequested = false;
  let activePollController = null;
  let wakePollWait = null;
  let commandQueue = Promise.resolve();
  let updateQueue = Promise.resolve();
  const confirmationErrors = new Map([
    ["ensure_identity", "Plugin did not confirm the identity command"],
    ["create_identity", "Plugin did not confirm identity creation"],
    ["restore_identity", "Plugin did not confirm identity recovery"],
    ["restore_remote_identity", "Plugin did not confirm remote identity recovery"],
    ["submit_action", "Plugin did not confirm the table action"],
    ["leave_table", "Plugin did not confirm the leave-table command"],
  ]);
  const sessionController = new AbortController();
  const app = new apps.App(
    { name: "token-holdem", version: ${JSON.stringify(version)} },
    { availableDisplayModes: ["inline", "fullscreen"] },
    { autoResize: false },
  );
  globalThis.__TOKEN_HOLDEM_MCP_APP__ = app;

  function errorMessage(error) {
    return error instanceof Error ? error.message : String(error);
  }

  function sessionStoppedError(reason) {
    const error = new Error(reason || "Token Poker UI session stopped");
    error.name = "SessionStoppedError";
    return error;
  }

  function isExpectedStop(error) {
    return stopped || error?.name === "SessionStoppedError" ||
      sessionController.signal.aborted;
  }

  function isTerminalHostError(error) {
    return /thread not found|resource.*(?:teardown|unmounted)|session.*(?:closed|disposed|terminated)|transport.*(?:closed|disconnected)|connection.*(?:closed|disposed)|protocol.*(?:closed|disconnected)/iu.test(
      errorMessage(error),
    );
  }

  function stopResizeNotifications() {
    if (typeof resizeCleanup !== "function") return;
    const cleanup = resizeCleanup;
    resizeCleanup = null;
    try {
      cleanup();
    } catch (error) {
      console.warn("Token Poker could not stop inline resize notifications", error);
    }
  }

  function syncResizeNotifications(displayMode) {
    if (!displayModeSettled || stopped || viewSuspended) return;
    if (displayMode === "inline") {
      if (resizeCleanup === null && !resizeSetupFailed) {
        try {
          resizeCleanup = app.setupSizeChangedNotifications();
        } catch (error) {
          resizeSetupFailed = true;
          publishWarning("Inline resize notifications are unavailable: " + errorMessage(error));
        }
      }
      return;
    }
    resizeSetupFailed = false;
    stopResizeNotifications();
  }

  function stopSession(reason) {
    if (stopped) return;
    stopped = true;
    stopResizeNotifications();
    globalThis.removeEventListener("pagehide", handlePageHide);
    globalThis.removeEventListener("pageshow", handlePageShow);
    document.removeEventListener?.("visibilitychange", handleVisibilityChange);
    document.removeEventListener?.("freeze", handlePageHide);
    document.removeEventListener?.("resume", handlePageShow);
    app.removeEventListener("hostcontextchanged", applyHostContext);
    app.removeEventListener("toolresult", consumeHostResult);
    sessionController.abort(sessionStoppedError(reason));
  }

  function handlePageHide() {
    if (stopped || viewSuspended) return;
    viewSuspended = true;
    stopResizeNotifications();
  }

  function handlePageShow() {
    if (stopped || !viewSuspended) return;
    viewSuspended = false;
    pollRefreshRequested = true;
    if (activePollController !== null) {
      const error = new Error("Token Poker view resumed; refreshing the current state");
      error.name = "PollResumedError";
      activePollController.abort(error);
    }
    wakePollWait?.();
    // Coalesce painting, not visibility state: another hide/show can happen
    // before the animation frame scheduled by the previous resume runs.
    if (resumeScheduled) return;
    resumeScheduled = true;
    const refresh = () => {
      resumeScheduled = false;
      if (stopped || viewSuspended) return;
      applyHostContext(app.getHostContext?.());
      globalThis.dispatchEvent(new CustomEvent("token-holdem:resume", {
        detail: { latestSequence },
      }));
    };
    if (typeof globalThis.requestAnimationFrame === "function") {
      globalThis.requestAnimationFrame(refresh);
    } else {
      globalThis.setTimeout(refresh, 0);
    }
  }

  function handleVisibilityChange() {
    if (document.visibilityState === "hidden") handlePageHide();
    else handlePageShow();
  }

  globalThis.addEventListener("pagehide", handlePageHide);
  globalThis.addEventListener("pageshow", handlePageShow);
  document.addEventListener?.("visibilitychange", handleVisibilityChange);
  document.addEventListener?.("freeze", handlePageHide);
  document.addEventListener?.("resume", handlePageShow);

  function publishSidecarEvent(detail) {
    if (detail && detail.type === "token_snapshot_accepted") {
      publishAccountBinding({
        account_fingerprint: detail.account_fingerprint,
        peer_verifiable: detail.peer_verifiable,
      });
    }
    const buffer = globalThis.__tokenHoldemBufferedSidecarEvents;
    buffer.push(detail);
    if (buffer.length > 4_096) buffer.splice(0, buffer.length - 4_096);
    globalThis.dispatchEvent(new CustomEvent("token-holdem:sidecar", {
      detail,
    }));
  }

  function publishAccountBinding(detail) {
    if (!detail || typeof detail !== "object" ||
        typeof detail.account_fingerprint !== "string" || detail.account_fingerprint.length === 0 ||
        typeof detail.peer_verifiable !== "boolean") return;
    globalThis.__tokenHoldemLastAccountBinding = detail;
    globalThis.dispatchEvent(new CustomEvent("token-holdem:account-binding", {
      detail,
    }));
  }

  function publishCurrentState(detail) {
    if (!detail || typeof detail !== "object" ||
        !(detail.identity === null ||
          (typeof detail.identity === "object" && !Array.isArray(detail.identity)))) return;
    globalThis.__tokenHoldemCurrentState = detail;
    globalThis.dispatchEvent(new CustomEvent("token-holdem:current-state", {
      detail,
    }));
  }

  function publishWarning(message) {
    publishSidecarEvent({ type: "warning", message: String(message) });
  }

  function publishFatal(message) {
    const detail = String(message);
    globalThis.__tokenHoldemBootError = detail;
    globalThis.dispatchEvent(new CustomEvent("token-holdem:fatal", { detail }));
  }

  function publishOfficialUsageState(phase, error = null) {
    const state = { phase, error: phase === "error" ? String(error) : null };
    globalThis.__tokenHoldemOfficialUsageState = state;
    globalThis.dispatchEvent(new CustomEvent("token-holdem:official-usage-status", {
      detail: state,
    }));
  }

  function publishUpdateStatus(detail) {
    if (!detail || typeof detail !== "object") return;
    globalThis.__tokenPokerUpdateStatus = detail;
    globalThis.dispatchEvent(new CustomEvent("token-poker:update-status", { detail }));
  }

  function publishPendingUpdatePhase(phase) {
    publishUpdateStatus({
      ...globalThis.__tokenPokerUpdateStatus,
      phase,
      error: null,
    });
  }

  function resultError(result) {
    const text = Array.isArray(result?.content)
      ? result.content.find((item) => item?.type === "text")?.text
      : null;
    return typeof text === "string" && text.length > 0 ? text : "Plugin tool call failed";
  }

  function isRecord(value) {
    return value !== null && typeof value === "object" && !Array.isArray(value);
  }

  function resultPayload(result) {
    if (!isRecord(result)) return null;
    if (isRecord(result.structuredContent)) return result.structuredContent;
    return isRecord(result._meta?.widgetData) ? result._meta.widgetData : null;
  }

  function validatePollPayload(result) {
    const payload = resultPayload(result);
    if (!payload || !Number.isSafeInteger(payload.latest_sequence) || payload.latest_sequence < 0 ||
        !Array.isArray(payload.events)) {
      throw new Error("Invalid Token Poker poll response: missing event payload or cursor. Retrying synchronization.");
    }
    function validEntries(entries) {
      return Array.isArray(entries) && entries.every((entry) =>
        isRecord(entry) && Number.isSafeInteger(entry.sequence) && entry.sequence >= 0 &&
        entry.sequence <= payload.latest_sequence && isRecord(entry.event) &&
        typeof entry.event.type === "string",
      );
    }
    if (!validEntries(payload.events) || (payload.current_state !== undefined &&
        (!isRecord(payload.current_state) ||
          payload.current_state.latest_sequence !== payload.latest_sequence ||
          !validEntries(payload.current_state.events)))) {
      throw new Error("Invalid Token Poker poll response: inconsistent events or current-state projection. Retrying synchronization.");
    }
    return payload;
  }

  function consumeHostResult(result) {
    // A restored Codex tab can replay results from a previous MCP process.
    // Use one cached result for initial presentation only; it must never move
    // the live event cursor or retire the current server's stream.
    if (stopped || bootstrapConsumed || liveStateReceived || resultPayload(result) === null) return;
    bootstrapConsumed = true;
    consumeResult(result, "metadata", true);
  }

  function consumeResult(result, eventMode = eventsHydrated ? "incremental" : "metadata", bootstrap = false) {
    if (!isRecord(result)) return false;
    const payload = resultPayload(result);
    const incomingStream = payload?.current_state?.stream_id;
    if (!bootstrap && typeof incomingStream === "string" && incomingStream !== streamId) {
      if (retiredStreams.has(incomingStream)) return false;
      if (streamId !== null) retiredStreams.add(streamId);
      streamId = incomingStream;
      latestSequence = 0;
      latestPayloadSequence = -1;
    }
    const payloadSequence = payload?.current_state?.latest_sequence;
    if (!bootstrap && Number.isSafeInteger(payloadSequence)) {
      if (payloadSequence < latestPayloadSequence) return false;
      latestPayloadSequence = payloadSequence;
    }
    if (!bootstrap && payload !== null) liveStateReceived = true;
    const officialUsageError = payload && typeof payload === "object" &&
      typeof payload.official_usage_error === "string" && payload.official_usage_error.length > 0
        ? payload.official_usage_error
        : null;
    if (officialUsageError !== null) {
      publishOfficialUsageState("error", officialUsageError);
      publishWarning(officialUsageError);
    }
    if (payload && typeof payload === "object" && payload.token_snapshot && typeof payload.token_snapshot === "object") {
      globalThis.__tokenHoldemLastSnapshot = payload.token_snapshot;
      publishOfficialUsageState("ready");
      globalThis.dispatchEvent(new CustomEvent("token-holdem:snapshot", {
        detail: payload.token_snapshot,
      }));
    }
    if (payload && typeof payload === "object") {
      publishAccountBinding(payload.account_binding);
      publishUpdateStatus(payload.update_status);
    }
    if (result.isError) {
      publishCurrentState(payload?.current_state);
      if (officialUsageError === null) publishWarning(resultError(result));
      return false;
    }
    if (payload === null) return false;
    if (eventMode === "metadata") {
      publishCurrentState(payload.current_state);
      return true;
    }
    if (Array.isArray(payload.events)) {
      for (const entry of payload.events) {
        if (!entry || !Number.isSafeInteger(entry.sequence) || entry.sequence <= latestSequence) continue;
        latestSequence = entry.sequence;
        publishSidecarEvent(entry.event);
      }
    }
    if (Number.isSafeInteger(payload.latest_sequence)) {
      latestSequence = Math.max(latestSequence, payload.latest_sequence);
    }
    publishCurrentState(payload.current_state);
    if (payload.history_truncated === true) {
      publishWarning("Older diagnostic events were truncated; the current identity, matchmaking, room, and hand state was restored from the retained projection.");
    }
    return true;
  }

  async function callTool(name, argumentsValue, timeoutMs = 30_000, requestSignal = null) {
    await app.ready;
    if (stopped) throw sessionStoppedError("Token Poker UI session already stopped");
    const callController = new AbortController();
    const timeoutError = new Error("Plugin tool call timed out: " + name);
    timeoutError.name = "TimeoutError";
    const forwardSessionAbort = () => {
      callController.abort(sessionController.signal.reason ?? sessionStoppedError());
    };
    const forwardRequestAbort = () => callController.abort(requestSignal.reason);
    if (sessionController.signal.aborted) {
      forwardSessionAbort();
    } else {
      sessionController.signal.addEventListener("abort", forwardSessionAbort, { once: true });
    }
    if (requestSignal?.aborted) forwardRequestAbort();
    else requestSignal?.addEventListener("abort", forwardRequestAbort, { once: true });
    let rejectOnAbort;
    const aborted = new Promise((_, reject) => {
      rejectOnAbort = () => reject(callController.signal.reason ?? sessionStoppedError());
      if (callController.signal.aborted) rejectOnAbort();
      else callController.signal.addEventListener("abort", rejectOnAbort, { once: true });
    });
    const timeout = setTimeout(() => callController.abort(timeoutError), timeoutMs);
    try {
      const result = await Promise.race([
        app.callServerTool(
          { name, arguments: argumentsValue },
          { signal: callController.signal },
        ),
        aborted,
      ]);
      // Polling validates the complete batch before applying it or advancing
      // hydration. Other tools keep their command-specific result contracts.
      if (name !== "token_holdem_poll") consumeResult(result);
      return result;
    } finally {
      clearTimeout(timeout);
      sessionController.signal.removeEventListener("abort", forwardSessionAbort);
      requestSignal?.removeEventListener("abort", forwardRequestAbort);
      callController.signal.removeEventListener("abort", rejectOnAbort);
    }
  }

  async function refreshOfficialUsage() {
    publishOfficialUsageState("loading");
    try {
      await callTool("token_holdem_refresh_official_usage", {});
    } catch (error) {
      if (isExpectedStop(error)) return;
      publishOfficialUsageState("error", error instanceof Error ? error.message : String(error));
      throw error;
    }
  }

  async function runUpdateTool(name, pendingPhase, timeoutMs) {
    publishPendingUpdatePhase(pendingPhase);
    try {
      await callTool(name, {}, timeoutMs);
    } catch (error) {
      if (isExpectedStop(error)) return;
      publishUpdateStatus({
        ...globalThis.__tokenPokerUpdateStatus,
        phase: "error",
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }

  function createRequestId() {
    if (typeof globalThis.crypto?.randomUUID === "function") {
      return globalThis.crypto.randomUUID();
    }
    const bytes = new Uint8Array(16);
    if (typeof globalThis.crypto?.getRandomValues === "function") {
      globalThis.crypto.getRandomValues(bytes);
    } else {
      for (let index = 0; index < bytes.length; index += 1) {
        bytes[index] = Math.floor(Math.random() * 256);
      }
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    const hex = [...bytes].map((value) => value.toString(16).padStart(2, "0")).join("");
    return hex.slice(0, 8) + "-" + hex.slice(8, 12) + "-" + hex.slice(12, 16) + "-" +
      hex.slice(16, 20) + "-" + hex.slice(20);
  }

  function enqueue(queueName, task) {
    const queue = queueName === "update" ? updateQueue : commandQueue;
    const operation = queue.then(task);
    const continuation = operation.catch(() => undefined);
    if (queueName === "update") updateQueue = continuation;
    else commandQueue = continuation;
    return operation;
  }

  function failedCommand(error) {
    const message = errorMessage(error);
    if (!isExpectedStop(error)) publishWarning(message);
    return { ok: false, error: message };
  }

  async function submitCommand(command) {
    const requestId = createRequestId();
    const result = await callTool("token_holdem_command", {
      request_id: requestId,
      command,
    });
    const payload = resultPayload(result);
    const outcome = payload?.command_result;
    if (!outcome || outcome.request_id !== requestId) {
      throw new Error("Plugin returned a mismatched command result");
    }
    if (result?.isError || outcome.status === "failed") {
      return {
        ok: false,
        error: typeof outcome.error === "string" && outcome.error.length > 0
          ? outcome.error
          : resultError(result),
      };
    }
    const confirmationError = confirmationErrors.get(command?.type);
    if (confirmationError && outcome.status !== "confirmed") {
      throw new Error(confirmationError);
    }
    if (outcome.status !== "accepted" && outcome.status !== "confirmed") {
      throw new Error("Plugin returned an invalid command status");
    }
    if (["ensure_identity", "create_identity", "restore_identity", "restore_remote_identity"].includes(command?.type)) {
      const identity = outcome.identity_confirmation;
      if (!identity || typeof identity.player_id !== "string" || identity.player_id.length === 0 ||
          typeof identity.recovery_envelope !== "string" || !identity.recovery_envelope.startsWith("THR1-") ||
          identity.account_fingerprint !== command.expected_account_fingerprint ||
          typeof identity.recovery_secret_confirmed !== "boolean") {
        throw new Error("Plugin returned an invalid identity recovery confirmation");
      }
      return { ok: true, identity: { playerId: identity.player_id,
        recoveryEnvelope: identity.recovery_envelope,
        accountFingerprint: identity.account_fingerprint, recoverySecretConfirmed: identity.recovery_secret_confirmed } };
    }
    return { ok: true };
  }

  globalThis.tokenHoldemCommand = (payload) => {
    let command;
    try {
      command = JSON.parse(payload);
    } catch {
      const error = "The table sent an invalid command";
      publishWarning(error);
      return Promise.resolve({ ok: false, error });
    }
    if (command?.type === "close_ui") {
      return app.requestTeardown({})
        .then(() => ({ ok: true }))
        .catch(failedCommand);
    }
    if (stopped) {
      return Promise.resolve({ ok: false, error: "Token Poker UI session already stopped" });
    }
    if (command?.type === "request_token_refresh") {
      return enqueue("command", refreshOfficialUsage)
        .then(() => ({ ok: true }))
        .catch(failedCommand);
    }
    const updateTools = {
      check_update: ["token_holdem_check_update", "checking", 45_000],
      prepare_update: ["token_holdem_prepare_update", "downloading", 5 * 60_000],
      install_update: ["token_holdem_install_update", "installing", 6 * 60_000],
    };
    if (Object.prototype.hasOwnProperty.call(updateTools, command?.type)) {
      const [toolName, pendingPhase, timeoutMs] = updateTools[command.type];
      return enqueue("update", () => runUpdateTool(toolName, pendingPhase, timeoutMs))
        .then(() => ({ ok: true }))
        .catch(failedCommand);
    }
    return enqueue("command", () => submitCommand(command)).catch(failedCommand);
  };

  function abortableDelay(delayMs) {
    if (stopped) return Promise.resolve();
    return new Promise((resolveDelay) => {
      // A null delay parks the single poll loop while its view is hidden.
      const timeout = delayMs === null ? null : setTimeout(finish, delayMs);
      function finish() {
        if (timeout !== null) clearTimeout(timeout);
        if (wakePollWait === finish) wakePollWait = null;
        sessionController.signal.removeEventListener("abort", finish);
        resolveDelay();
      }
      wakePollWait = finish;
      sessionController.signal.addEventListener("abort", finish, { once: true });
    });
  }

  async function pollLoop() {
    let retryDelayMs = 1_000;
    let hasPolled = false;
    while (!stopped) {
      if (viewSuspended && hasPolled) await abortableDelay(null);
      if (stopped) return;
      const startedAt = Date.now();
      try {
        const isHydrating = !eventsHydrated;
        const refreshImmediately = isHydrating || pollRefreshRequested;
        pollRefreshRequested = false;
        const beforeSequence = latestSequence;
        const beforeStream = streamId;
        let result;
        const pollController = new AbortController();
        activePollController = pollController;
        hasPolled = true;
        try {
          result = await callTool(
            "token_holdem_poll",
            { after_sequence: isHydrating ? 0 : latestSequence, wait_ms: refreshImmediately ? 0 : 20_000 },
            30_000,
            pollController.signal,
          );
        } finally {
          if (activePollController === pollController) activePollController = null;
        }
        if (result?.isError) throw new Error(resultError(result));
        validatePollPayload(result);
        if (!consumeResult(result, isHydrating ? "hydrate" : "incremental")) {
          throw new Error("Token Poker poll returned an obsolete stream or cursor. Retrying synchronization.");
        }
        eventsHydrated = true;
        retryDelayMs = 1_000;
        // Even a nominally successful host response can arrive immediately
        // without progress. Yield to painting and bound traffic in that case.
        const progressed = latestSequence > beforeSequence || streamId !== beforeStream;
        const minimumInterval = progressed ? (isHydrating ? 0 : 100) : 1_000;
        const remainingDelay = minimumInterval - (Date.now() - startedAt);
        if (remainingDelay > 0) {
          await abortableDelay(remainingDelay);
        }
      } catch (error) {
        if (isExpectedStop(error)) return;
        if (error?.name === "PollResumedError") continue;
        if (isTerminalHostError(error)) {
          publishFatal(errorMessage(error));
          stopSession(errorMessage(error));
          return;
        }
        publishWarning(errorMessage(error));
        await abortableDelay(retryDelayMs);
        retryDelayMs = Math.min(15_000, retryDelayMs * 2);
      }
    }
  }

  function applyHostContext(context) {
    if (!context) return;
    if (displayModeSettled) {
      syncResizeNotifications(context.displayMode ?? app.getHostContext?.()?.displayMode);
    }
    try {
      if (context.theme && typeof apps.applyDocumentTheme === "function") {
        apps.applyDocumentTheme(context.theme);
      }
      if (context.styles?.variables && typeof apps.applyHostStyleVariables === "function") {
        apps.applyHostStyleVariables(context.styles.variables);
      }
      if (context.styles?.css?.fonts && typeof apps.applyHostFonts === "function") {
        apps.applyHostFonts(context.styles.css.fonts);
      }
    } catch {
      return;
    }
  }

  async function settleDisplayMode(initialContext) {
    if (initialContext?.displayMode === "fullscreen") {
      displayModeSettled = true;
      return;
    }
    try {
      const result = await app.requestDisplayMode(
        { mode: "fullscreen" },
        { signal: sessionController.signal, timeout: 5_000 },
      );
      if (stopped) return;
      displayModeSettled = true;
      syncResizeNotifications(result?.mode ?? app.getHostContext?.()?.displayMode);
    } catch (error) {
      if (isExpectedStop(error)) return;
      displayModeSettled = true;
      syncResizeNotifications(app.getHostContext?.()?.displayMode ?? initialContext?.displayMode);
      publishWarning("Codex could not change the table display mode: " + errorMessage(error));
    }
  }

  app.addEventListener("hostcontextchanged", applyHostContext);
  app.addEventListener("toolresult", consumeHostResult);
  app.onteardown = async () => {
    stopSession("Codex host tore down the Token Poker UI");
    return {};
  };
  app.ready = app.connect()
    .then(() => {
      if (stopped) return;
      const initialContext = app.getHostContext?.();
      applyHostContext(initialContext);
      // Fullscreen is a presentation preference, not a prerequisite for data.
      // Re-requesting it on restored fullscreen tabs also needlessly remounts
      // the host container while React is initializing.
      void settleDisplayMode(initialContext);
      updateQueue = updateQueue.then(() =>
        runUpdateTool("token_holdem_check_update", "checking", 45_000),
      );
      void pollLoop();
    })
    .catch((error) => {
      if (!isExpectedStop(error)) {
        publishFatal(errorMessage(error));
        publishWarning(errorMessage(error));
      }
      stopSession(errorMessage(error));
    });
})();`;
}

function escapeInlineScript(source) {
  return source.replaceAll("</script", "<\\/script").replaceAll("</SCRIPT", "<\\/SCRIPT");
}
