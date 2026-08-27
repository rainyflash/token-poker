import assert from "node:assert/strict";
import { randomBytes, randomUUID } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { SidecarRuntime } from "../src/sidecar-runtime.mjs";

const pluginRoot = resolve(import.meta.dirname, "..", "..");
const mcpRoot = join(pluginRoot, "mcp");

test("新对话 MCP 工具恢复旧对话的共享运行时状态", async () => {
  const isolatedLocalAppData = await mkdtemp(join(tmpdir(), "token-holdem-mcp-reattach-"));
  const environmentSnapshot = snapshotEnvironment([
    "LOCALAPPDATA",
    "TOKEN_HOLDEM_RUNTIME_PATH",
    "TOKEN_HOLDEM_SIDECAR_PATH",
    "TOKEN_HOLDEM_RUNTIME_PIPE",
    "TOKEN_HOLDEM_RUNTIME_IDLE_TIMEOUT_SECONDS",
    "TOKEN_HOLDEM_CODEX_APP_SERVER_FIXTURE",
  ]);
  const overrides = {
    LOCALAPPDATA: isolatedLocalAppData,
    TOKEN_HOLDEM_RUNTIME_PATH: join(pluginRoot, "bin", "token-holdem-runtime.exe"),
    TOKEN_HOLDEM_SIDECAR_PATH: join(pluginRoot, "bin", "token-holdem-sidecar.exe"),
    TOKEN_HOLDEM_RUNTIME_PIPE: String.raw`\\.\pipe\token-holdem-runtime-v6-${randomBytes(12).toString("hex")}`,
    TOKEN_HOLDEM_RUNTIME_IDLE_TIMEOUT_SECONDS: "30",
    TOKEN_HOLDEM_CODEX_APP_SERVER_FIXTURE: join(
      mcpRoot,
      "test",
      "fixtures",
      "mock-codex-app-server.mjs",
    ),
  };
  Object.assign(process.env, overrides);
  const environment = Object.fromEntries(
    Object.entries({ ...process.env, ...overrides, CODEX_MCP_NODE_PATH: process.execPath }).filter(
      (entry) => typeof entry[1] === "string",
    ),
  );

  let firstSession = null;
  let secondSession = null;
  try {
    await readFile(join(mcpRoot, "server.bundle.mjs"));
    firstSession = await connectMcpSession(environment, "first-conversation");
    const synchronized = await firstSession.client.callTool({
      name: "token_holdem_open",
      arguments: {},
    });
    const runtimeId = synchronized.structuredContent?.runtime_id;
    assert.equal(typeof runtimeId, "string");

    const identityCommand = await firstSession.client.callTool({
      name: "token_holdem_command",
      arguments: {
        request_id: randomUUID(),
        command: {
          type: "ensure_identity",
          recovery_secret: "跨任务恢复测试专用口令-abcdefghijkl",
          device_label: "测试设备",
        },
      },
    });
    assert.equal(identityCommand.structuredContent?.command_result?.status, "confirmed");
    const identityEvent = await findOrWaitForRuntimeEvent(
      firstSession.client,
      identityCommand,
      "identity_ready",
    );
    assert.equal(identityEvent.type, "identity_ready");

    const poolCommand = await firstSession.client.callTool({
      name: "token_holdem_command",
      arguments: {
        request_id: randomUUID(),
        command: {
          type: "join_public_pool",
          level_id: "1m-2m",
          buy_in: 80_000_000,
        },
      },
    });
    const poolEvent = await findOrWaitForRuntimeEvent(
      firstSession.client,
      poolCommand,
      "pool_joined",
    );
    assert.deepEqual(
      {
        level_id: poolEvent.level_id,
        buy_in: poolEvent.buy_in,
      },
      { level_id: "1m-2m", buy_in: 80_000_000 },
    );

    await closeMcpSession(firstSession);
    firstSession = null;

    secondSession = await connectMcpSession(environment, "second-conversation");
    const reopened = await secondSession.client.callTool({
      name: "token_holdem_open",
      arguments: {},
    });
    assert.equal(reopened.structuredContent?.runtime_id, runtimeId);
    const replayedPoolEvent = reopened.structuredContent?.events?.find(
      (entry) => entry.event?.type === "pool_joined",
    )?.event;
    assert.deepEqual(
      {
        level_id: replayedPoolEvent?.level_id,
        buy_in: replayedPoolEvent?.buy_in,
      },
      { level_id: "1m-2m", buy_in: 80_000_000 },
    );
    assert.equal(reopened.structuredContent?.token_snapshot?.lifetime_tokens, 35_500_000_000);
    assert.equal(reopened.structuredContent?.token_snapshot?.username, "player");
    assert.equal(reopened.structuredContent?.token_snapshot?.display_name, null);
    assert.equal(
      reopened.structuredContent?.token_snapshot?.source,
      "codex_app_server_account_usage",
    );
    assert.equal(
      typeof reopened.structuredContent?.account_binding?.account_fingerprint,
      "string",
    );
    assert.equal(reopened.structuredContent?.account_binding?.peer_verifiable, false);
    assert.equal(
      reopened.structuredContent?.current_state?.identity?.player_id,
      identityEvent.player_id,
    );
    assert.equal(
      reopened.structuredContent?.current_state?.latest_sequence,
      reopened.structuredContent?.latest_sequence,
    );
    assert.equal(
      reopened.structuredContent?.current_state?.events?.some(
        (entry) => entry.event?.type === "pool_joined",
      ),
      true,
    );
  } finally {
    await closeMcpSession(firstSession);
    await closeMcpSession(secondSession);
    const runtime = new SidecarRuntime(pluginRoot, "test-build");
    await runtime.terminateRuntimeForTesting().catch(() => undefined);
    restoreEnvironment(environmentSnapshot);
    await rm(isolatedLocalAppData, { recursive: true, force: true });
  }
});

async function connectMcpSession(environment, name) {
  const transport = new StdioClientTransport({
    command: process.env.ComSpec ?? "cmd.exe",
    args: ["/d", "/s", "/c", "call", "./scripts/launch-mcp.cmd", "./mcp/server.bundle.mjs"],
    cwd: pluginRoot,
    env: environment,
    stderr: "pipe",
  });
  const client = new Client(
    { name, version: "0.1.3" },
    {
      capabilities: {
        extensions: {
          "io.modelcontextprotocol/ui": {
            mimeTypes: ["text/html;profile=mcp-app"],
          },
        },
      },
    },
  );
  await client.connect(transport);
  return { client, transport };
}

async function closeMcpSession(session) {
  if (session === null) return;
  await session.client.close().catch(() => undefined);
  await session.transport.close().catch(() => undefined);
}

async function findOrWaitForRuntimeEvent(client, initialResult, eventType) {
  const initialEvents = initialResult.structuredContent?.events ?? [];
  const initialMatch = initialEvents.find((entry) => entry.event?.type === eventType);
  if (initialMatch !== undefined) return initialMatch.event;

  let cursor = initialResult.structuredContent?.latest_sequence ?? 0;
  for (let attempt = 0; attempt < 8; attempt += 1) {
    const result = await client.callTool({
      name: "token_holdem_poll",
      arguments: { after_sequence: cursor, wait_ms: 1_000 },
    });
    const events = result.structuredContent?.events ?? [];
    const matched = events.find((entry) => entry.event?.type === eventType);
    if (matched !== undefined) return matched.event;
    cursor = result.structuredContent?.latest_sequence ?? cursor;
  }
  throw new Error(`等待共享运行时事件超时：${eventType}`);
}

function snapshotEnvironment(names) {
  return new Map(names.map((name) => [name, process.env[name]]));
}

function restoreEnvironment(snapshot) {
  for (const [name, value] of snapshot) {
    if (value === undefined) delete process.env[name];
    else process.env[name] = value;
  }
}
