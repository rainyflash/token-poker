import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  createWindowsPowerShellEnvironment,
  InstallerLauncher,
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
  let spawnedOptions = null;
  const launcher = new InstallerLauncher({
    powershellResolver: async () => "powershell.exe",
    spawnImpl: (_executable, argumentsList, options) => {
      spawnedArguments = argumentsList;
      spawnedOptions = options;
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
  assert.equal(spawnedOptions.detached, false);
});

test("waits for a result that becomes visible shortly after process exit", async (context) => {
  const directory = await mkdtemp(join(tmpdir(), "token-poker-delayed-result-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const resultPath = join(directory, "update-result.json");
  const child = new EventEmitter();
  child.unref = () => undefined;
  const launcher = new InstallerLauncher({
    powershellResolver: async () => "powershell.exe",
    spawnImpl: () => {
      setImmediate(() => {
        child.emit("exit", 0, null);
        setTimeout(() => {
          void writeInstallerResult(resultPath, "0.4.7", "succeeded");
        }, 25);
      });
      return child;
    },
    resultTimeoutMs: 500,
    timeoutMs: 1_000,
  });

  const result = await launcher.launch({
    release: {
      version: "0.4.7",
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
});

test("reports process exit details when no result is produced", async (context) => {
  const directory = await mkdtemp(join(tmpdir(), "token-poker-missing-result-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const child = new EventEmitter();
  child.unref = () => undefined;
  const launcher = new InstallerLauncher({
    powershellResolver: async () => "powershell.exe",
    spawnImpl: () => {
      setImmediate(() => child.emit("exit", 9, null));
      return child;
    },
    resultTimeoutMs: 25,
    timeoutMs: 1_000,
  });
  const logPath = join(directory, "install.log");

  await assert.rejects(
    launcher.launch({
      release: {
        version: "0.4.7",
        artifact: { sha256: "a".repeat(64), bytes: 42 },
      },
      prepared: {
        helperPath: join(directory, "apply-update.ps1"),
        archivePath: join(directory, "package.zip"),
        resultPath: join(directory, "update-result.json"),
        logPath,
      },
      parentProcessId: 123,
    }),
    new RegExp(`exited with code 9.*${escapeRegExp(logPath)}`, "u"),
  );
});

test("removes PowerShell 7 module paths before launching Windows PowerShell", () => {
  const sanitized = createWindowsPowerShellEnvironment({
    SystemRoot: "C:\\Windows",
    ProgramFiles: "C:\\Program Files",
    PSModulePath: "C:\\Codex\\PowerShell\\Modules",
    TOKEN_POKER_SENTINEL: "preserved",
  });

  assert.equal(sanitized.TOKEN_POKER_SENTINEL, "preserved");
  assert.equal(
    sanitized.PSModulePath,
    [
      "C:\\Program Files\\WindowsPowerShell\\Modules",
      "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\Modules",
    ].join(";"),
  );
  assert.doesNotMatch(sanitized.PSModulePath, /Codex\\PowerShell/u);
});

test(
  "runs the real Windows PowerShell helper with a poisoned parent module path",
  { skip: process.platform !== "win32" },
  async (context) => {
    const directory = await mkdtemp(join(tmpdir(), "token-poker-real-powershell-"));
    context.after(() => rm(directory, { recursive: true, force: true }));
    const poisonedModulePath = join(directory, "PowerShell", "Modules");
    await mkdir(poisonedModulePath, { recursive: true });
    const archivePath = join(directory, "package.zip");
    const resultPath = join(directory, "update-result.json");
    const helperPath = join(directory, "apply-update.ps1");
    await writeFile(archivePath, "payload", "utf8");
    await writeFile(
      helperPath,
      windowsPowerShellProbeScript(),
      "utf8",
    );
    const systemRoot = process.env.SystemRoot;
    assert.equal(typeof systemRoot, "string");
    const launcher = new InstallerLauncher({
      environment: { ...process.env, PSModulePath: poisonedModulePath },
      powershellResolver: async () =>
        join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe"),
      resultTimeoutMs: 2_000,
      timeoutMs: 10_000,
    });

    const result = await launcher.launch({
      release: {
        version: "0.4.7",
        artifact: { sha256: "a".repeat(64), bytes: 7 },
      },
      prepared: {
        helperPath,
        archivePath,
        resultPath,
        logPath: join(directory, "install.log"),
      },
      parentProcessId: process.pid,
    });

    assert.equal(result.status, "succeeded");
  },
);

function writeInstallerResult(path, version, status) {
  return writeFile(
    path,
    `${JSON.stringify({
      schema_version: 1,
      version,
      status,
      message: "installed",
      completed_at_unix_ms: 1_700_000_000_000,
    })}\n`,
    "utf8",
  );
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

function windowsPowerShellProbeScript() {
  return String.raw`param(
    [string]$ArchivePath,
    [string]$ExpectedVersion,
    [string]$ExpectedSha256,
    [long]$ExpectedBytes,
    [int]$ParentProcessId,
    [int]$DelaySeconds
)
$ErrorActionPreference = 'Stop'
Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256 | Out-Null
$resultPath = Join-Path (Split-Path -Parent $ArchivePath) 'update-result.json'
@{
    schema_version = 1
    version = $ExpectedVersion
    status = 'succeeded'
    message = 'installed'
    completed_at_unix_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
} | ConvertTo-Json | Set-Content -LiteralPath $resultPath -Encoding UTF8
`;
}
