import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  DetachedInstallerLauncher,
  readInstallerResult,
} from "../src/update/installer-launcher.mjs";

test("accepts only a completed result for the requested release", async (context) => {
  const directory = await mkdtemp(join(tmpdir(), "token-poker-installer-result-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const resultPath = join(directory, "update-result.json");
  await writeFile(
    resultPath,
    `\uFEFF${JSON.stringify({
      schema_version: 1,
      version: "0.4.4",
      status: "succeeded",
      message: "installed",
      completed_at_unix_ms: 1_700_000_000_000,
    })}\n`,
    "utf8",
  );

  const result = await readInstallerResult(resultPath, "0.4.4");
  assert.equal(result.status, "succeeded");
  assert.equal(result.version, "0.4.4");
});

test("rejects a stale result from another release", async (context) => {
  const directory = await mkdtemp(join(tmpdir(), "token-poker-stale-result-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const resultPath = join(directory, "update-result.json");
  await writeFile(
    resultPath,
    `${JSON.stringify({
      schema_version: 1,
      version: "0.4.3",
      status: "succeeded",
      message: "stale",
      completed_at_unix_ms: 1_700_000_000_000,
    })}\n`,
    "utf8",
  );

  await assert.rejects(readInstallerResult(resultPath, "0.4.4"), /requested release/u);
});

test("waits for the isolated installer and forwards its verified result", async (context) => {
  const directory = await mkdtemp(join(tmpdir(), "token-poker-launcher-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const resultPath = join(directory, "update-result.json");
  await writeFile(
    resultPath,
    `${JSON.stringify({
      schema_version: 1,
      version: "0.4.4",
      status: "succeeded",
      message: "installed",
      completed_at_unix_ms: 1_700_000_000_000,
    })}\n`,
    "utf8",
  );
  const child = new EventEmitter();
  child.unref = () => undefined;
  let spawnedArguments = null;
  const launcher = new DetachedInstallerLauncher({
    powershellResolver: async () => "powershell.exe",
    spawnImpl: (_executable, argumentsList) => {
      spawnedArguments = argumentsList;
      setImmediate(() => child.emit("exit", 0, null));
      return child;
    },
    timeoutMs: 1_000,
  });

  const result = await launcher.launch({
    release: {
      version: "0.4.4",
      artifact: { sha256: "a".repeat(64), bytes: 42 },
    },
    prepared: {
      helperPath: join(directory, "apply-update.ps1"),
      archivePath: join(directory, "package.zip"),
      resultPath,
      logPath: join(directory, "install.log"),
    },
    parentProcessId: 123,
  });

  assert.equal(result.status, "succeeded");
  assert.deepEqual(spawnedArguments.slice(-2), ["-DelaySeconds", "0"]);
});
