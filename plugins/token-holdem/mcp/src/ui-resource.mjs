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
  let stopped = false;
  let resizeCleanup = null;
  let resizeSetupFailed = false;
  let displayModeSettled = false;
  let viewSuspended = document.visibilityState === "hidden";
  let resumeScheduled = false;
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
    app.removeEventListener("toolresult", consumeResult);
    sessionController.abort(sessionStoppedError(reason));
  }

  function handlePageHide() {
    if (stopped || viewSuspended) return;
    viewSuspended = true;
    stopResizeNotifications();
  }

  function handlePageShow() {
    if (stopped || !viewSuspended || resumeScheduled) return;
    viewSuspended = false;
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

  function consumeResult(result, eventMode = eventsHydrated ? "incremental" : "metadata") {
    if (!result || typeof result !== "object") return;
    const payload = result.structuredContent || result._meta?.widgetData;
    const incomingStream = payload?.current_state?.stream_id;
    if (typeof incomingStream === "string" && incomingStream !== streamId) {
      if (retiredStreams.has(incomingStream)) return;
      if (streamId !== null) retiredStreams.add(streamId);
      streamId = incomingStream;
      latestSequence = 0;
      latestPayloadSequence = -1;
    }
    const payloadSequence = payload?.current_state?.latest_sequence;
    if (Number.isSafeInteger(payloadSequence)) {
      if (payloadSequence < latestPayloadSequence) return;
      latestPayloadSequence = payloadSequence;
    }
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
      return;
    }
    if (!payload || typeof payload !== "object") return;
    if (eventMode === "metadata") {
      publishCurrentState(payload.current_state);
      return;
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
  }

  async function callTool(name, argumentsValue, timeoutMs = 30_000, eventMode = "auto") {
    await app.ready;
    if (stopped) throw sessionStoppedError("Token Poker UI session already stopped");
    const callController = new AbortController();
    const timeoutError = new Error("Plugin tool call timed out: " + name);
    timeoutError.name = "TimeoutError";
    const forwardSessionAbort = () => {
      callController.abort(sessionController.signal.reason ?? sessionStoppedError());
    };
    if (sessionController.signal.aborted) {
      forwardSessionAbort();
    } else {
      sessionController.signal.addEventListener("abort", forwardSessionAbort, { once: true });
    }
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
      consumeResult(
        result,
        eventMode === "auto" ? (eventsHydrated ? "incremental" : "metadata") : eventMode,
      );
      return result;
    } finally {
      clearTimeout(timeout);
      sessionController.signal.removeEventListener("abort", forwardSessionAbort);
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
    const payload = result?.structuredContent || result?._meta?.widgetData;
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
      const timeout = setTimeout(finish, delayMs);
      function finish() {
        clearTimeout(timeout);
        sessionController.signal.removeEventListener("abort", finish);
        resolveDelay();
      }
      sessionController.signal.addEventListener("abort", finish, { once: true });
    });
  }

  async function pollLoop() {
    while (!stopped) {
      try {
        const isHydrating = !eventsHydrated;
        const result = await callTool(
          "token_holdem_poll",
          { after_sequence: isHydrating ? 0 : latestSequence, wait_ms: isHydrating ? 0 : 20_000 },
          30_000,
          isHydrating ? "hydrate" : "incremental",
        );
        if (result?.isError) {
          const message = resultError(result);
          if (isTerminalHostError(message)) {
            publishFatal(message);
            stopSession(message);
            return;
          }
          await abortableDelay(1_000);
        } else if (isHydrating) {
          eventsHydrated = true;
        }
      } catch (error) {
        if (isExpectedStop(error)) return;
        if (isTerminalHostError(error)) {
          publishFatal(errorMessage(error));
          stopSession(errorMessage(error));
          return;
        }
        publishWarning(errorMessage(error));
        await abortableDelay(1_000);
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

  app.addEventListener("hostcontextchanged", applyHostContext);
  app.addEventListener("toolresult", consumeResult);
  app.onteardown = async () => {
    stopSession("Codex host tore down the Token Poker UI");
    return {};
  };
  app.ready = app.connect()
    .then(async () => {
      if (stopped) return;
      const initialContext = app.getHostContext?.();
      applyHostContext(initialContext);
      const displayResult = await app.requestDisplayMode({ mode: "fullscreen" }).catch(() => null);
      displayModeSettled = true;
      syncResizeNotifications(displayResult?.mode ?? initialContext?.displayMode);
      if (stopped) return;
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
