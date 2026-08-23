import { spawn } from "node:child_process";
import { access, readFile } from "node:fs/promises";
import { join } from "node:path";

const DEFAULT_INSTALL_TIMEOUT_MS = 5 * 60_000;

export class DetachedInstallerLauncher {
  #spawn;
  #resolvePowerShell;
  #timeoutMs;

  constructor({
    spawnImpl = spawn,
    powershellResolver = resolvePowerShell,
    timeoutMs = DEFAULT_INSTALL_TIMEOUT_MS,
  } = {}) {
    if (typeof spawnImpl !== "function") throw new Error("A process launcher is required");
    if (typeof powershellResolver !== "function") {
      throw new Error("A PowerShell resolver is required");
    }
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
      throw new Error("The installer timeout must be a positive integer");
    }
    this.#spawn = spawnImpl;
    this.#resolvePowerShell = powershellResolver;
    this.#timeoutMs = timeoutMs;
  }

  async launch({ release, prepared, parentProcessId }) {
    const powershell = await this.#resolvePowerShell();
    const child = this.#spawn(
      powershell,
      [
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        prepared.helperPath,
        "-ArchivePath",
        prepared.archivePath,
        "-ExpectedVersion",
        release.version,
        "-ExpectedSha256",
        release.artifact.sha256,
        "-ExpectedBytes",
        String(release.artifact.bytes),
        "-ParentProcessId",
        String(parentProcessId),
        "-DelaySeconds",
        "0",
      ],
      {
        detached: true,
        stdio: "ignore",
        windowsHide: true,
      },
    );
    const processResult = await waitForInstallerExit(child, this.#timeoutMs);
    const result = await readInstallerResult(prepared.resultPath, release.version);
    if (processResult.code !== 0 || result.status !== "succeeded") {
      const detail = result.message.length > 0 ? result.message : "The installer failed";
      throw new Error(`${detail} See ${prepared.logPath}`);
    }
    return result;
  }
}

export async function readInstallerResult(path, expectedVersion) {
  let value;
  try {
    const rawResult = await readFile(path, "utf8");
    value = JSON.parse(rawResult.replace(/^\uFEFF/u, ""));
  } catch (error) {
    const detail = error instanceof Error ? error.message : "unknown read error";
    throw new Error(`The updater did not produce a valid result: ${detail}`);
  }
  if (
    typeof value !== "object" ||
    value === null ||
    value.schema_version !== 1 ||
    value.version !== expectedVersion ||
    !["validated", "succeeded", "failed"].includes(value.status) ||
    typeof value.message !== "string" ||
    !Number.isSafeInteger(value.completed_at_unix_ms)
  ) {
    throw new Error("The updater result does not match the requested release");
  }
  return Object.freeze({
    status: value.status,
    version: value.version,
    message: value.message,
    completedAtUnixMs: value.completed_at_unix_ms,
  });
}

function waitForInstallerExit(child, timeoutMs) {
  return new Promise((resolveExit, rejectExit) => {
    let settled = false;
    const finish = (operation) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      child.off("error", onError);
      child.off("exit", onExit);
      operation();
    };
    const onError = (error) => finish(() => rejectExit(error));
    const onExit = (code, signal) => finish(() => resolveExit({ code, signal }));
    const timer = setTimeout(() => {
      finish(() => {
        child.unref();
        rejectExit(new Error("The verified installer did not finish within five minutes"));
      });
    }, timeoutMs);
    child.once("error", onError);
    child.once("exit", onExit);
  });
}

async function resolvePowerShell() {
  const systemRoot = process.env.SystemRoot;
  if (typeof systemRoot === "string" && systemRoot.length > 0) {
    const candidate = join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
    try {
      await access(candidate);
      return candidate;
    } catch {
      // The command lookup fallback remains necessary on non-standard Windows images.
    }
  }
  return "powershell.exe";
}
