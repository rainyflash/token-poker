import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import {
  CodexAccountUsageReader,
  resolveCodexAppServerLaunchPlan,
} from "../src/codex-account-usage.mjs";
import { OfficialTokenService } from "../src/official-token-service.mjs";

const fixture = resolve(import.meta.dirname, "fixtures", "mock-codex-app-server.mjs");

test("通过官方 App Server 协议读取账户累计 Token", async () => {
  const reader = new CodexAccountUsageReader({
    launchPlan: { executable: process.execPath, args: [fixture] },
  });
  const result = await reader.read();

  assert.equal(result.lifetimeTokens, 35_500_000_000);
  assert.equal(result.accountIdentifier, "chatgpt-email:player@example.com");
  assert.equal(result.username, "player");
  assert.equal(result.displayName, null);
  assert.equal(result.source, "codex_app_server_account_usage");
  assert.equal(Number.isSafeInteger(result.observedAtUnixMs), true);
});

test("旧 App Server 返回明确的更新提示", async () => {
  const previousMode = process.env.TOKEN_HOLDEM_TEST_APP_SERVER_MODE;
  process.env.TOKEN_HOLDEM_TEST_APP_SERVER_MODE = "unsupported";
  try {
    const reader = new CodexAccountUsageReader({
      launchPlan: { executable: process.execPath, args: [fixture] },
    });
    await assert.rejects(reader.read(), /版本过旧/u);
  } finally {
    if (previousMode === undefined) delete process.env.TOKEN_HOLDEM_TEST_APP_SERVER_MODE;
    else process.env.TOKEN_HOLDEM_TEST_APP_SERVER_MODE = previousMode;
  }
});

test("安装器准备的本地 App Server 优先于商店包路径", () => {
  assert.deepEqual(
    resolveCodexAppServerLaunchPlan({
      TOKEN_HOLDEM_CODEX_APP_SERVER_PATH: String.raw`C:\插件\bin\codex-app-server.exe`,
      CODEX_CLI_PATH: String.raw`C:\Program Files\WindowsApps\OpenAI.Codex\codex.exe`,
    }),
    {
      executable: String.raw`C:\插件\bin\codex-app-server.exe`,
      args: ["app-server"],
    },
  );
});

test("Windows 商店包执行受限时返回可操作的修复提示", async () => {
  const accessError = Object.assign(new Error("spawn EPERM"), { code: "EPERM" });
  const reader = new CodexAccountUsageReader({
    launchPlan: { executable: "codex.exe", args: ["app-server"] },
    spawnProcess() {
      throw accessError;
    },
  });

  await assert.rejects(reader.read(), /install-token-poker\.ps1 -Upgrade/u);
});

test("官方 Token 用例合并并发刷新并向领域运行时发布规范化快照", async () => {
  let reads = 0;
  const commands = [];
  const reader = {
    async read() {
      reads += 1;
      return {
        lifetimeTokens: 35_500_000_000,
        accountIdentifier: "chatgpt-email:player@example.com",
        observedAtUnixMs: 1_000,
        source: "codex_app_server_account_usage",
      };
    },
  };
  const runtime = {
    tokenSnapshot: null,
    accountBinding: null,
    async publishTokenSnapshot(command) {
      commands.push(command);
      const accepted = {
        type: "token_snapshot_accepted",
        account_fingerprint: "8f3c58d35e4a9db2b6a00d54a1a8d88a7ab3e9114f8924a7ce5510d23f9b8af6",
        peer_verifiable: false,
      };
      this.accountBinding = {
        account_fingerprint: accepted.account_fingerprint,
        peer_verifiable: accepted.peer_verifiable,
      };
      return accepted;
    },
  };
  const service = new OfficialTokenService({ reader, runtime, now: () => 1_000 });

  const [first, second] = await Promise.all([
    service.refresh({ force: true }),
    service.refresh({ force: true }),
  ]);
  await service.refresh();

  assert.equal(reads, 1);
  assert.equal(first, second);
  assert.equal(commands.length, 1);
  assert.deepEqual(commands[0], {
    type: "token_snapshot",
    lifetime_tokens: 35_500_000_000,
    username: null,
    display_name: null,
    account_identifier: "chatgpt-email:player@example.com",
    observed_at_unix_ms: 1_000,
    source: "codex_app_server_account_usage",
  });
});
