import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
  registerAppResource,
  registerAppTool,
  RESOURCE_MIME_TYPE,
} from "@modelcontextprotocol/ext-apps/server";
import { CodexAccountUsageReader } from "./codex-account-usage.mjs";
import { dispatchHostCommand } from "./command-router.mjs";
import { OfficialTokenService } from "./official-token-service.mjs";
import { SidecarRuntime } from "./sidecar-runtime.mjs";
import {
  commandToolSchema,
  pollToolSchema,
  refreshOfficialUsageToolSchema,
} from "./tool-contracts.mjs";
import { buildTokenHoldemHtml } from "./ui-resource.mjs";
import { createUpdateService } from "./update/index.mjs";

const moduleDirectory = dirname(fileURLToPath(import.meta.url));
const pluginRoot = resolve(moduleDirectory, "..");
const manifest = JSON.parse(
  await readFile(join(pluginRoot, ".codex-plugin", "plugin.json"), "utf8"),
);
const APP_URI = "ui://token-holdem/table.html";
const runtime = new SidecarRuntime(pluginRoot, manifest.version);
const officialTokens = new OfficialTokenService({
  reader: new CodexAccountUsageReader({ clientVersion: manifest.version }),
  runtime,
});
const updates = createUpdateService({ currentVersion: manifest.version, pluginRoot });
const FRESH_USAGE_COMMANDS = new Set([
  "join_public_pool",
  "create_friend_room",
  "join_friend_room",
  "ensure_identity",
  "create_identity",
  "restore_identity",
  "restore_remote_identity",
]);

const server = new McpServer(
  { name: "token-holdem", version: manifest.version },
  {
    instructions:
      "Token Poker is peer-to-peer poker inside Codex. Each Codex task may attach to the same per-user shared runtime; another task or agent does not exclusively own it. Opening the table automatically reads lifetime Token through the official Codex App Server. Never open Settings, transcribe, or estimate Token. This server-side statistic is not a signed balance proof that opponents can verify independently.",
  },
);

const appHtml = await buildTokenHoldemHtml(pluginRoot, manifest.version);

registerAppResource(
  server,
  "Token Poker table",
  APP_URI,
  {
    title: "Token Poker",
    description: "A Codex-style peer-to-peer Mental Poker table.",
    _meta: {
      ui: {
        prefersBorder: false,
        csp: {
          resourceDomains: ["data:", "blob:"],
          connectDomains: [],
        },
      },
    },
  },
  async () => ({
    contents: [
      {
        uri: APP_URI,
        mimeType: RESOURCE_MIME_TYPE,
        text: appHtml,
        _meta: {
          ui: {
            prefersBorder: false,
            csp: {
              resourceDomains: ["data:", "blob:"],
              connectDomains: [],
            },
          },
        },
      },
    ],
  }),
);

registerAppTool(
  server,
  "token_holdem_open",
  {
    title: "Open or resume Token Poker",
    description:
      "Open Token Poker inside Codex. If the per-user shared runtime already has a game or another Codex task is attached, reattach without exclusive ownership and restore it while reading official lifetime Token automatically.",
    annotations: {
      readOnlyHint: false,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: false,
    },
    _meta: appToolMeta(["model", "app"], "Opening table…", "Table opened"),
  },
  async () => {
    await runtime.ensureStarted();
    const usageError = await refreshUsageError(true);
    return toolResult(
      usageError === null
        ? "Token Poker connected to the shared game runtime and read official Codex lifetime Token."
        : `Token Poker connected to the shared game runtime, but official lifetime Token could not be refreshed: ${usageError}`,
      { ...currentPayload("open"), official_usage_error: usageError },
    );
  },
);

registerAppTool(
  server,
  "token_holdem_refresh_official_usage",
  {
    title: "Refresh official Codex lifetime Token",
    description:
      "Read lifetime Token for the current ChatGPT account directly through the official Codex App Server, without opening Settings or accepting manual input.",
    inputSchema: refreshOfficialUsageToolSchema,
    annotations: {
      readOnlyHint: false,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: false,
    },
    _meta: appToolMeta(["model", "app"], "Reading official lifetime Token…", "Official lifetime Token refreshed"),
  },
  async () => {
    const usageError = await refreshUsageError(true);
    if (usageError !== null) {
      return toolResult(
        `Could not read official lifetime Token: ${usageError}`,
        { ...currentPayload("official_usage_refresh"), official_usage_error: usageError },
        { isError: true },
      );
    }
    const snapshot = currentTokenObservation();
    if (snapshot === null) {
      throw new Error("The official Token service completed without producing a snapshot");
    }
    return toolResult(
      `Refreshed ${String(snapshot.lifetime_tokens)} Token from the official Codex account-usage API.`,
      currentPayload("official_usage_refresh"),
    );
  },
);

registerAppTool(
  server,
  "token_holdem_command",
  {
    title: "Token Poker table command",
    description: "Local command channel used only by the isolated table UI.",
    inputSchema: commandToolSchema,
    annotations: {
      readOnlyHint: false,
      destructiveHint: false,
      idempotentHint: false,
      openWorldHint: false,
    },
    _meta: appToolMeta(["app"], "Submitting table command…", "Table command submitted"),
  },
  async ({ request_id: requestId, command }) => {
    if (FRESH_USAGE_COMMANDS.has(command.type)) {
      const usageError = await refreshUsageError(false);
      if (usageError !== null) {
        return toolResult(
          `Table command stopped: ${usageError}`,
          {
            ...currentPayload("command"),
            official_usage_error: usageError,
            command_result: commandResult(requestId, "failed", usageError),
          },
          { isError: true },
        );
      }
    }
    const beforeSequence = runtime.latestSequence;
    try {
      const status = await dispatchHostCommand(runtime, command, requestId);
      return toolResult(
        status === "confirmed"
          ? "Table command was confirmed by the local game core."
          : "Table command was passed to the local game core.",
        {
          ...payloadAfter("command", beforeSequence),
          command_result: commandResult(requestId, status),
        },
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : "Table command failed";
      return toolResult(
        `Table command failed: ${message}`,
        {
          ...payloadAfter("command", beforeSequence),
          command_result: commandResult(requestId, "failed", message),
        },
        { isError: true },
      );
    }
  },
);

registerAppTool(
  server,
  "token_holdem_poll",
  {
    title: "Token Poker live events",
    description: "Long-polls local game events for the isolated table UI only.",
    inputSchema: pollToolSchema,
    annotations: {
      readOnlyHint: true,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: false,
    },
    _meta: appToolMeta(["app"], "Waiting for game events…", "Game events updated"),
  },
  async ({ after_sequence: afterSequence, wait_ms: waitMs }) => {
    const eventBatch = await runtime.waitForEvents(afterSequence, waitMs);
    return toolResult("Game events updated.", payloadFromEventBatch("poll", eventBatch));
  },
);

registerAppTool(
  server,
  "token_holdem_check_update",
  {
    title: "Check for Token Poker updates",
    description: "Read and validate the stable release manifest from the official GitHub repository.",
    annotations: {
      readOnlyHint: true,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: true,
    },
    _meta: appToolMeta(["app"], "Checking for updates…", "Update check completed"),
  },
  async () => {
    const status = await updates.check();
    const message =
      status.phase === "available"
        ? `Token Poker ${status.latest_version} is available.`
        : status.phase === "current"
          ? `Token Poker ${manifest.version} is current.`
          : `Could not check for Token Poker updates: ${status.error}`;
    return toolResult(message, currentPayload("update_check"));
  },
);

registerAppTool(
  server,
  "token_holdem_prepare_update",
  {
    title: "Download and verify a Token Poker update",
    description:
      "Download the selected stable Windows package into local staging and verify its declared size and SHA-256 digest.",
    annotations: {
      readOnlyHint: false,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: true,
    },
    _meta: appToolMeta(["app"], "Downloading verified update…", "Update package verified"),
  },
  async () => {
    const status = await updates.prepare();
    return toolResult(
      status.phase === "ready"
        ? `Token Poker ${status.latest_version} was downloaded and verified.`
        : `Could not prepare the Token Poker update: ${status.error}`,
      currentPayload("update_prepare"),
    );
  },
);

registerAppTool(
  server,
  "token_holdem_install_update",
  {
    title: "Install the verified Token Poker update",
    description:
      "Hand the verified package to an isolated updater, replace the plugin through Codex CLI, and require a Codex restart.",
    annotations: {
      readOnlyHint: false,
      destructiveHint: true,
      idempotentHint: false,
      openWorldHint: false,
    },
    _meta: appToolMeta(["app"], "Starting verified update…", "Updater started"),
  },
  async () => {
    const status = await updates.install();
    return toolResult(
      status.phase === "restart_required"
        ? "Token Poker was installed and verified. Restart Codex to load the new version."
        : `Could not start the Token Poker update: ${status.error}`,
      currentPayload("update_install"),
    );
  },
);

const transport = new StdioServerTransport();
await server.connect(transport);

let shuttingDown = false;
async function shutdown() {
  if (shuttingDown) return;
  shuttingDown = true;
  await runtime.close().catch((error) => {
    process.stderr.write(`[token-holdem-mcp] failed to disconnect shared game runtime: ${error.message}\n`);
  });
  await server.close().catch(() => undefined);
}

process.once("SIGINT", () => void shutdown());
process.once("SIGTERM", () => void shutdown());
process.stdin.once("end", () => void shutdown());
process.stdin.once("close", () => void shutdown());

function currentPayload(action) {
  return payloadFromEventBatch(action, runtime.readEvents(0));
}

function payloadAfter(action, afterSequence) {
  return payloadFromEventBatch(action, runtime.readEvents(afterSequence));
}

function payloadFromEventBatch(action, eventBatch) {
  return {
    version: 1,
    action,
    token_snapshot: currentTokenObservation(),
    account_binding: runtime.accountBinding,
    update_status: updates.snapshot,
    ...eventBatch,
  };
}

function currentTokenObservation() {
  return officialTokens.snapshot;
}

async function refreshUsageError(force) {
  try {
    await officialTokens.refresh({ force });
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : "Could not read official Codex lifetime Token";
  }
}

function toolResult(message, payload, { isError = false } = {}) {
  return {
    content: [{ type: "text", text: message }],
    structuredContent: payload,
    ...(isError ? { isError: true } : {}),
    _meta: {
      "openai/outputTemplate": APP_URI,
      widgetData: payload,
    },
  };
}

function commandResult(requestId, status, error = null) {
  return Object.freeze({
    request_id: requestId,
    status,
    error,
  });
}

function appToolMeta(visibility, invoking, invoked) {
  return {
    ui: { resourceUri: APP_URI, visibility },
    "openai/outputTemplate": APP_URI,
    "openai/widgetAccessible": true,
    "openai/toolInvocation/invoking": invoking,
    "openai/toolInvocation/invoked": invoked,
  };
}
