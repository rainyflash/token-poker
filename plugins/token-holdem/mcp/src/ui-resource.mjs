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
  let stopped = false;
  let resizeCleanup = null;
  let resizeSetupFailed = false;
  let displayModeSettled = false;
  let commandQueue = Promise.resolve();
  let updateQueue = Promise.resolve();
  const sessionController = new AbortController();
  const app = new apps.App(
    { name: "token-holdem", version: ${JSON.stringify(version)} },
    { availableDisplayModes: ["inline", "pip", "fullscreen"] },
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
    if (!displayModeSettled || stopped) return;
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
    app.removeEventListener("hostcontextchanged", applyHostContext);
    app.removeEventListener("toolresult", consumeResult);
    sessionController.abort(sessionStoppedError(reason));
  }

  function handlePageHide() {
    stopSession("Token Poker iframe was unloaded");
  }

  globalThis.addEventListener("pagehide", handlePageHide, { once: true });

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

  function consumeResult(result) {
    if (!result || typeof result !== "object") return;
    const payload = result.structuredContent || result._meta?.widgetData;
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
      if (officialUsageError === null) publishWarning(resultError(result));
      return;
    }
    if (!payload || typeof payload !== "object") return;
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
    if (payload.history_truncated === true) {
      publishWarning("Local event history was truncated. Continuing from the latest state; reopen the table if the UI looks stale.");
    }
  }

  async function callTool(name, argumentsValue, timeoutMs = 30_000) {
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
      consumeResult(result);
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

  globalThis.tokenHoldemCommand = (payload) => {
    let command;
    try {
      command = JSON.parse(payload);
    } catch {
      publishWarning("The table sent an invalid command");
      return;
    }
    if (command?.type === "close_ui") {
      app.requestTeardown({}).catch((error) => {
        if (!isExpectedStop(error)) publishWarning(errorMessage(error));
      });
      return;
    }
    if (stopped) return;
    if (command?.type === "request_token_refresh") {
      commandQueue = commandQueue
        .then(refreshOfficialUsage)
        .catch((error) => {
          if (!isExpectedStop(error)) publishWarning(errorMessage(error));
        });
      return;
    }
    const updateTools = {
      check_update: ["token_holdem_check_update", "checking", 45_000],
      prepare_update: ["token_holdem_prepare_update", "downloading", 5 * 60_000],
      install_update: ["token_holdem_install_update", "installing", 6 * 60_000],
    };
    if (Object.prototype.hasOwnProperty.call(updateTools, command?.type)) {
      const [toolName, pendingPhase, timeoutMs] = updateTools[command.type];
      updateQueue = updateQueue.then(() => runUpdateTool(toolName, pendingPhase, timeoutMs));
      return;
    }
    commandQueue = commandQueue
      .then(() => callTool("token_holdem_command", { command }))
      .catch((error) => {
        if (!isExpectedStop(error)) publishWarning(errorMessage(error));
      });
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
        const result = await callTool(
          "token_holdem_poll",
          { after_sequence: latestSequence, wait_ms: 20_000 },
          30_000,
        );
        if (result?.isError) {
          const message = resultError(result);
          if (isTerminalHostError(message)) {
            publishFatal(message);
            stopSession(message);
            return;
          }
          await abortableDelay(1_000);
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
