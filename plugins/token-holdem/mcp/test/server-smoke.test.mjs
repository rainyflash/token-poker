import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const sourceMcpRoot = resolve(import.meta.dirname, "..");
const pluginRoot = process.env.TOKEN_HOLDEM_TEST_PLUGIN_ROOT
  ? resolve(process.env.TOKEN_HOLDEM_TEST_PLUGIN_ROOT)
  : resolve(sourceMcpRoot, "..");
const mcpRoot = join(pluginRoot, "mcp");

test("Agent workflow discovers lazy plugin tools before diagnosing a load failure", async () => {
  const skill = await readFile(
    join(pluginRoot, "skills", "token-holdem", "SKILL.md"),
    "utf8",
  );

  assert.match(skill, /mcp__token_holdem__token_holdem_open/u);
  assert.match(skill, /deferred-tool discovery/u);
  assert.match(skill, /does not reserve, lock, or consume the tool or runtime/u);
  assert.match(skill, /Classify failures by the layer that actually failed/u);
  assert.match(skill, /Never emit a prewritten compatibility diagnosis/u);
});

test("发布载荷清单覆盖 MCP UI 的全部磁盘依赖", async () => {
  const releaseContract = JSON.parse(
    await readFile(join(pluginRoot, "release-files.json"), "utf8"),
  );
  const releaseFiles = new Set(releaseContract.files);
  const requiredFiles = [
    "mcp/server.bundle.mjs",
    "mcp/vendor/ext-apps-app-with-deps.js",
    "scripts/apply-update.ps1",
    "ui/token-holdem.css",
    "ui/token-holdem.js",
  ];

  assert.equal(releaseContract.schema_version, 1);
  for (const relativePath of requiredFiles) {
    assert.equal(releaseFiles.has(relativePath), true, `发布载荷遗漏 ${relativePath}`);
    await readFile(join(pluginRoot, ...relativePath.split("/")));
  }
});

test("官方插件通过 MCP 读取官方账户用量并返回隔离牌桌资源", async (context) => {
  const isolatedLocalAppData = await mkdtemp(join(tmpdir(), "token-holdem-mcp-test-"));
  const serverPath = join(mcpRoot, "server.bundle.mjs");
  await readFile(serverPath);
  const nodeOverride =
    process.env.TOKEN_HOLDEM_TEST_USE_CODEX_RUNTIME === "1"
      ? {}
      : { CODEX_MCP_NODE_PATH: process.execPath };
  const environment = Object.fromEntries(
    Object.entries({
      ...process.env,
      LOCALAPPDATA: isolatedLocalAppData,
      ...nodeOverride,
      TOKEN_HOLDEM_SIDECAR_PATH: join(pluginRoot, "bin", "token-holdem-sidecar.exe"),
      TOKEN_HOLDEM_RUNTIME_PATH: join(pluginRoot, "bin", "token-holdem-runtime.exe"),
      TOKEN_HOLDEM_RUNTIME_PIPE: String.raw`\\.\pipe\token-holdem-runtime-v3-${randomBytes(12).toString("hex")}`,
      TOKEN_HOLDEM_RUNTIME_IDLE_TIMEOUT_SECONDS: "1",
      TOKEN_HOLDEM_CODEX_APP_SERVER_FIXTURE: join(
        sourceMcpRoot,
        "test",
        "fixtures",
        "mock-codex-app-server.mjs",
      ),
    }).filter((entry) => typeof entry[1] === "string"),
  );
  const transport = new StdioClientTransport({
    command: process.env.ComSpec ?? "cmd.exe",
    args: [
      "/d",
      "/s",
      "/c",
      "call",
      "./scripts/launch-mcp.cmd",
      "./mcp/server.bundle.mjs",
    ],
    cwd: pluginRoot,
    env: environment,
    stderr: "pipe",
  });
  const client = new Client(
    { name: "token-holdem-smoke-test", version: "0.1.3" },
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
  context.after(async () => {
    await client.close().catch(() => undefined);
    await transport.close().catch(() => undefined);
    await new Promise((resolveWait) => setTimeout(resolveWait, 1_500));
    await rm(isolatedLocalAppData, { recursive: true, force: true });
  });

  await client.connect(transport);
  const tools = await client.listTools();
  const toolsByName = new Map(tools.tools.map((tool) => [tool.name, tool]));
  assert.match(
    toolsByName.get("token_holdem_open")?.description ?? "",
    /without exclusive ownership/u,
  );
  assert.deepEqual(
    toolsByName.get("token_holdem_refresh_official_usage")?._meta?.ui?.visibility,
    ["model", "app"],
  );
  assert.equal(toolsByName.has("token_holdem_sync_official_tokens"), false);
  assert.deepEqual(toolsByName.get("token_holdem_command")?._meta?.ui?.visibility, ["app"]);
  assert.deepEqual(toolsByName.get("token_holdem_check_update")?._meta?.ui?.visibility, ["app"]);
  assert.deepEqual(toolsByName.get("token_holdem_prepare_update")?._meta?.ui?.visibility, ["app"]);
  assert.deepEqual(toolsByName.get("token_holdem_install_update")?._meta?.ui?.visibility, ["app"]);

  const resource = await client.readResource({ uri: "ui://token-holdem/table.html" });
  const html = resource.contents[0]?.text;
  assert.equal(typeof html, "string");
  assert.match(html, /__tokenHoldemBridgeInstalled/u);
  assert.match(html, /token_holdem_poll/u);
  assert.match(html, /token-poker-boot-status/u);
  assert.match(html, /autoResize: false/u);
  assert.match(html, /app\.onteardown/u);

  const synchronized = await client.callTool({
    name: "token_holdem_open",
    arguments: {},
  });
  assert.equal(synchronized.isError, undefined);
  assert.equal(synchronized.structuredContent?.token_snapshot?.lifetime_tokens, 35_500_000_000);
  assert.equal(
    synchronized.structuredContent?.token_snapshot?.source,
    "codex_app_server_account_usage",
  );
  assert.equal(
    typeof synchronized.structuredContent?.account_binding?.account_fingerprint,
    "string",
  );
  assert.equal(synchronized.structuredContent?.account_binding?.peer_verifiable, false);

  let afterSequence = synchronized.structuredContent?.latest_sequence ?? 0;
  let accepted = synchronized.structuredContent?.events?.some(
    (entry) => entry.event?.type === "token_snapshot_accepted",
  );
  const deadline = Date.now() + 10_000;
  while (!accepted && Date.now() < deadline) {
    const polled = await client.callTool({
      name: "token_holdem_poll",
      arguments: { after_sequence: afterSequence, wait_ms: 2_000 },
    });
    assert.equal(polled.isError, undefined);
    accepted = polled.structuredContent?.events?.some(
      (entry) => entry.event?.type === "token_snapshot_accepted",
    );
    afterSequence = polled.structuredContent?.latest_sequence ?? afterSequence;
  }
  assert.equal(accepted, true, "sidecar 未确认官方 Token 快照");
});
