import { spawn } from "node:child_process";
import { access, readFile } from "node:fs/promises";
import { delimiter, join } from "node:path";

const DEFAULT_INSTALL_TIMEOUT_MS = 5 * 60_000;
const DEFAULT_RESULT_TIMEOUT_MS = 5_000;
const DEFAULT_RESULT_RETRY_DELAY_MS = 50;

export class InstallerLauncher {
  #environment;
  #spawn;
  #resolvePowerShell;
  #resultTimeoutMs;
  #timeoutMs;

  constructor({
    environment = process.env,
    spawnImpl = spawn,
    powershellResolver = () => resolvePowerShell(environment),
    resultTimeoutMs = DEFAULT_RESULT_TIMEOUT_MS,
    timeoutMs = DEFAULT_INSTALL_TIMEOUT_MS,
  } = {}) {
    if (typeof environment !== "object" || environment === null) {
      throw new Error("A process environment is required");
    }
    if (typeof spawnImpl !== "function") throw new Error("A process launcher is required");
    if (typeof powershellResolver !== "function") {
      throw new Error("A PowerShell resolver is required");
    }
    if (!Number.isSafeInteger(resultTimeoutMs) || resultTimeoutMs <= 0) {
      throw new Error("The installer result timeout must be a positive integer");
    }
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
      throw new Error("The installer timeout must be a positive integer");
    }
    this.#environment = Object.freeze({ ...environment });
    this.#spawn = spawnImpl;
    this.#resolvePowerShell = powershellResolver;
    this.#resultTimeoutMs = resultTimeoutMs;
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
        detached: false,
        env: createWindowsPowerShellEnvironment(this.#environment),
        stdio: "ignore",
        windowsHide: true,
      },
    );
    const processResult = await waitForInstallerExit(child, this.#timeoutMs);
    let result;
    try {
      result = await waitForInstallerResult(
        prepared.resultPath,
        release.version,
        this.#resultTimeoutMs,
      );
    } catch (error) {
      const detail = error instanceof Error ? error.message : "unknown result error";
      const processDetail = formatProcessExit(processResult);
      throw new Error(
        `The updater ${processDetail} without a valid result: ${detail}. See ${prepared.logPath}`,
      );
    }
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

export async function waitForInstallerResult(
  path,
  expectedVersion,
  timeoutMs = DEFAULT_RESULT_TIMEOUT_MS,
) {
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
    throw new Error("The installer result timeout must be a positive integer");
  }
  const deadline = Date.now() + timeoutMs;
  let latestError;
  do {
    try {
      return await readInstallerResult(path, expectedVersion);
    } catch (error) {
      latestError = error;
    }
    const remainingMs = deadline - Date.now();
    if (remainingMs <= 0) break;
    await delay(Math.min(DEFAULT_RESULT_RETRY_DELAY_MS, remainingMs));
  } while (Date.now() <= deadline);
  throw latestError;
}

export function createWindowsPowerShellEnvironment(environment = process.env) {
  const sanitized = { ...environment };
  for (const key of Object.keys(sanitized)) {
    if (key.toLowerCase() === "psmodulepath") delete sanitized[key];
  }
  const systemRoot = getEnvironmentValue(environment, "SystemRoot");
  if (systemRoot === null) return sanitized;

  const modulePaths = [];
  const programFiles = getEnvironmentValue(environment, "ProgramFiles");
  if (programFiles !== null) {
    modulePaths.push(join(programFiles, "WindowsPowerShell", "Modules"));
  }
  modulePaths.push(join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "Modules"));
  sanitized.PSModulePath = [...new Set(modulePaths)].join(delimiter);
  return sanitized;
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

function formatProcessExit({ code, signal }) {
  if (Number.isInteger(code)) return `exited with code ${code}`;
  if (typeof signal === "string" && signal.length > 0) {
    return `terminated after signal ${signal}`;
  }
  return "terminated";
}

function delay(milliseconds) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

function getEnvironmentValue(environment, name) {
  const key = Object.keys(environment).find(
    (candidate) => candidate.toLowerCase() === name.toLowerCase(),
  );
  const value = key === undefined ? undefined : environment[key];
  return typeof value === "string" && value.length > 0 ? value : null;
}

async function resolvePowerShell(environment = process.env) {
  const systemRoot = getEnvironmentValue(environment, "SystemRoot");
  if (systemRoot !== null) {
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
