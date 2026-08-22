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
      #token-holdem-portals { position: relative; z-index: 1000; }
      ${styleSource}
    </style>
    <script>${escapeInlineScript(appClient)}</script>
  </head>
  <body>
    <div id="token-holdem-root"></div>
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

function buildHostBridge(version) {
  return `(() => {
  "use strict";

  const apps = globalThis.__TOKEN_HOLDEM_MCP_APPS__;
  const mountRoot = document.getElementById("token-holdem-root");
  const portalRoot = document.getElementById("token-holdem-portals");
  if (!apps || typeof apps.App !== "function" || !mountRoot || !portalRoot) {
    throw new Error("Token Poker MCP host initialization failed");
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
  let commandQueue = Promise.resolve();
  let updateQueue = Promise.resolve();
  const app = new apps.App(
    { name: "token-holdem", version: ${JSON.stringify(version)} },
    { availableDisplayModes: ["inline", "pip", "fullscreen"] },
    { autoResize: true },
  );
  globalThis.__TOKEN_HOLDEM_MCP_APP__ = app;

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
    let timeout;
    const timer = new Promise((_, reject) => {
      timeout = setTimeout(() => reject(new Error("Plugin tool call timed out")), timeoutMs);
    });
    try {
      const result = await Promise.race([
        app.callServerTool({ name, arguments: argumentsValue }),
        timer,
      ]);
      consumeResult(result);
      return result;
    } finally {
      clearTimeout(timeout);
    }
  }

  async function refreshOfficialUsage() {
    publishOfficialUsageState("loading");
    try {
      await callTool("token_holdem_refresh_official_usage", {});
    } catch (error) {
      publishOfficialUsageState("error", error instanceof Error ? error.message : String(error));
      throw error;
    }
  }

  async function runUpdateTool(name, pendingPhase, timeoutMs) {
    publishPendingUpdatePhase(pendingPhase);
    try {
      await callTool(name, {}, timeoutMs);
    } catch (error) {
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
    if (command?.type === "request_token_refresh") {
      commandQueue = commandQueue
        .then(refreshOfficialUsage)
        .catch((error) => publishWarning(error instanceof Error ? error.message : String(error)));
      return;
    }
    const updateTools = {
      check_update: ["token_holdem_check_update", "checking", 45_000],
      prepare_update: ["token_holdem_prepare_update", "downloading", 5 * 60_000],
      install_update: ["token_holdem_install_update", "installing", 45_000],
    };
    if (Object.prototype.hasOwnProperty.call(updateTools, command?.type)) {
      const [toolName, pendingPhase, timeoutMs] = updateTools[command.type];
      updateQueue = updateQueue.then(() => runUpdateTool(toolName, pendingPhase, timeoutMs));
      return;
    }
    if (command?.type === "close_ui") {
      stopped = true;
      app.requestTeardown({}).catch(() => undefined);
      return;
    }
    commandQueue = commandQueue
      .then(() => callTool("token_holdem_command", { command }))
      .catch((error) => publishWarning(error instanceof Error ? error.message : String(error)));
  };

  async function pollLoop() {
    while (!stopped) {
      try {
        await callTool(
          "token_holdem_poll",
          { after_sequence: latestSequence, wait_ms: 20_000 },
          30_000,
        );
      } catch (error) {
        publishWarning(error instanceof Error ? error.message : String(error));
        await new Promise((resolveDelay) => setTimeout(resolveDelay, 1_000));
      }
    }
  }

  function applyHostContext(context) {
    if (!context) return;
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
  app.ready = app.connect()
    .then(async () => {
      applyHostContext(app.getHostContext?.());
      await app.requestDisplayMode({ mode: "fullscreen" }).catch(() => undefined);
      updateQueue = updateQueue.then(() =>
        runUpdateTool("token_holdem_check_update", "checking", 45_000),
      );
      void pollLoop();
    })
    .catch((error) => {
      publishWarning(error instanceof Error ? error.message : String(error));
      throw error;
    });
})();`;
}

function escapeInlineScript(source) {
  return source.replaceAll("</script", "<\\/script").replaceAll("</SCRIPT", "<\\/SCRIPT");
}
