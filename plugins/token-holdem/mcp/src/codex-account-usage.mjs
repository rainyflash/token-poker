import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { z } from "zod";

const DEFAULT_TIMEOUT_MS = 15_000;
const SHUTDOWN_GRACE_MS = 1_000;
const STDERR_LIMIT = 8_192;

const accountResponseSchema = z
  .object({
    account: z
      .object({
        type: z.string(),
        email: z.string().trim().min(1).max(320).nullable().optional(),
      })
      .passthrough()
      .nullable(),
  })
  .passthrough();

const usageResponseSchema = z
  .object({
    summary: z
      .object({
        lifetimeTokens: z
          .number()
          .int()
          .nonnegative()
          .max(Number.MAX_SAFE_INTEGER)
          .nullable(),
      })
      .passthrough(),
  })
  .passthrough();

export class CodexAccountUsageReader {
  #launchPlan;
  #timeoutMs;
  #spawnProcess;
  #clientVersion;

  constructor({
    launchPlan = resolveCodexAppServerLaunchPlan(),
    timeoutMs = DEFAULT_TIMEOUT_MS,
    spawnProcess = spawn,
    clientVersion = "0.0.0",
  } = {}) {
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 60_000) {
      throw new RangeError("Codex App Server 超时时间必须在 1 到 60000 毫秒之间");
    }
    this.#launchPlan = launchPlan;
    this.#timeoutMs = timeoutMs;
    this.#spawnProcess = spawnProcess;
    this.#clientVersion = normalizeClientVersion(clientVersion);
  }

  async read() {
    let transport = null;
    try {
      transport = new CodexAppServerTransport({
        ...this.#launchPlan,
        timeoutMs: this.#timeoutMs,
        spawnProcess: this.#spawnProcess,
        clientVersion: this.#clientVersion,
      });
      await transport.initialize();
      const [accountResult, usageResult] = await Promise.all([
        transport.request("account/read", { refreshToken: false }),
        transport.request("account/usage/read"),
      ]);
      return normalizeAccountUsage(accountResult, usageResult);
    } catch (error) {
      throw mapAccountUsageError(error);
    } finally {
      if (transport !== null) await transport.close();
    }
  }
}

export function resolveCodexAppServerLaunchPlan(environment = process.env) {
  const fixtureEntrypoint = cleanEnvironmentValue(
    environment.TOKEN_HOLDEM_CODEX_APP_SERVER_FIXTURE,
  );
  if (fixtureEntrypoint !== null) {
    return Object.freeze({ executable: process.execPath, args: [fixtureEntrypoint] });
  }

  const executable =
    cleanEnvironmentValue(environment.TOKEN_HOLDEM_CODEX_APP_SERVER_PATH) ??
    cleanEnvironmentValue(environment.TOKEN_HOLDEM_CODEX_CLI_PATH) ??
    cleanEnvironmentValue(environment.CODEX_CLI_PATH) ??
    "codex";
  return Object.freeze({ executable, args: ["app-server"] });
}

function normalizeAccountUsage(accountValue, usageValue) {
  const account = accountResponseSchema.parse(accountValue).account;
  const usage = usageResponseSchema.parse(usageValue);
  const lifetimeTokens = usage.summary.lifetimeTokens;
  if (lifetimeTokens === null) {
    throw new Error("Codex 官方服务尚未返回累计 Token，请稍后重试");
  }

  const email =
    account?.type === "chatgpt" && typeof account.email === "string"
      ? normalizeEmail(account.email)
      : null;
  return Object.freeze({
    lifetimeTokens,
    accountIdentifier: email === null ? null : `chatgpt-email:${email}`,
    username: email === null ? null : email.slice(0, email.lastIndexOf("@")),
    displayName: null,
    observedAtUnixMs: Date.now(),
    source: "codex_app_server_account_usage",
  });
}

function normalizeEmail(value) {
  const normalized = value.normalize("NFKC").trim().toLowerCase();
  if (normalized.length === 0 || normalized.length > 320 || !normalized.includes("@")) {
    throw new Error("Codex 账户返回了无效邮箱标识");
  }
  return normalized;
}

function cleanEnvironmentValue(value) {
  if (typeof value !== "string") return null;
  const normalized = value.trim().replace(/^"(.*)"$/su, "$1");
  return normalized.length === 0 ? null : normalized;
}

function normalizeClientVersion(value) {
  if (typeof value !== "string" || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/u.test(value)) {
    throw new TypeError("Codex App Server 客户端版本格式无效");
  }
  return value;
}

function mapAccountUsageError(error) {
  if (error instanceof Error && "code" in error) {
    if (error.code === "EPERM") {
      return new Error(
        "Windows 阻止直接启动 Codex 商店包中的 App Server；请重新运行 install-token-poker.ps1 -Upgrade 以准备插件本地运行副本",
        { cause: error },
      );
    }
    if (error.code === "ENOENT") {
      return new Error(
        "缺少 Codex App Server 本地运行副本；请重新运行 install-token-poker.ps1 -Upgrade",
        { cause: error },
      );
    }
  }
  if (error instanceof CodexRpcError) {
    if (
      error.code === -32601 ||
      /unknown variant [`']account\/usage\/read[`']/iu.test(error.message) ||
      /method not found/iu.test(error.message)
    ) {
      return new Error(
        "当前 Codex App Server 版本过旧，不支持官方累计 Token 接口；请更新 Codex 桌面端后重试",
        { cause: error },
      );
    }
    return new Error(`Codex 官方账户用量请求失败：${error.message}`, { cause: error });
  }
  if (error instanceof z.ZodError) {
    return new Error("Codex 官方账户用量响应格式不兼容", { cause: error });
  }
  return error instanceof Error
    ? error
    : new Error("读取 Codex 官方账户用量时发生未知错误");
}

class CodexRpcError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "CodexRpcError";
    this.code = code;
  }
}

class CodexAppServerTransport {
  #child;
  #clientVersion;
  #lines;
  #nextId = 0;
  #pending = new Map();
  #stderr = "";
  #timeoutMs;
  #closed = false;
  #exitPromise;

  constructor({ executable, args, timeoutMs, spawnProcess, clientVersion }) {
    this.#timeoutMs = timeoutMs;
    this.#clientVersion = clientVersion;
    this.#child = spawnProcess(executable, args, {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
      env: process.env,
    });
    this.#lines = createInterface({ input: this.#child.stdout, crlfDelay: Infinity });
    this.#lines.on("line", (line) => this.#acceptLine(line));
    this.#child.stderr.setEncoding("utf8");
    this.#child.stderr.on("data", (chunk) => {
      this.#stderr = `${this.#stderr}${String(chunk)}`.slice(-STDERR_LIMIT);
    });
    this.#child.once("error", (error) => this.#failAll(error));
    this.#exitPromise = new Promise((resolveExit) => {
      this.#child.once("exit", (code, signal) => {
        if (!this.#closed) {
          const suffix = this.#stderr.trim().length === 0 ? "" : `：${this.#stderr.trim()}`;
          this.#failAll(
            new Error(
              `Codex App Server 提前退出（code=${String(code)}, signal=${String(signal)}）${suffix}`,
            ),
          );
        }
        resolveExit();
      });
    });
  }

  async initialize() {
    await this.request("initialize", {
      clientInfo: {
        name: "token_holdem",
        title: "Token Poker",
        version: this.#clientVersion,
      },
    });
    await this.notify("initialized", {});
  }

  request(method, params) {
    const id = this.#nextId;
    this.#nextId += 1;
    const message = params === undefined ? { method, id } : { method, id, params };
    return new Promise((resolveRequest, rejectRequest) => {
      const timeout = setTimeout(() => {
        this.#pending.delete(id);
        rejectRequest(new Error(`Codex App Server 请求超时：${method}`));
      }, this.#timeoutMs);
      this.#pending.set(id, {
        resolve: (value) => {
          clearTimeout(timeout);
          resolveRequest(value);
        },
        reject: (error) => {
          clearTimeout(timeout);
          rejectRequest(error);
        },
      });
      this.#write(message).catch((error) => {
        const pending = this.#pending.get(id);
        this.#pending.delete(id);
        pending?.reject(error);
      });
    });
  }

  notify(method, params) {
    return this.#write({ method, params });
  }

  async close() {
    if (this.#closed) return;
    this.#closed = true;
    this.#lines.close();
    this.#failAll(new Error("Codex App Server 连接已关闭"));
    if (!this.#child.stdin.destroyed) this.#child.stdin.end();
    const exited = await Promise.race([
      this.#exitPromise.then(() => true),
      new Promise((resolveDelay) => setTimeout(() => resolveDelay(false), SHUTDOWN_GRACE_MS)),
    ]);
    if (!exited) this.#child.kill();
  }

  #acceptLine(line) {
    let message;
    try {
      message = JSON.parse(line);
    } catch (error) {
      this.#failAll(new Error("Codex App Server 返回了无效 JSON", { cause: error }));
      return;
    }
    if (typeof message !== "object" || message === null || !Number.isSafeInteger(message.id)) {
      return;
    }
    const pending = this.#pending.get(message.id);
    if (pending === undefined) return;
    this.#pending.delete(message.id);
    if (typeof message.error === "object" && message.error !== null) {
      const code = Number.isSafeInteger(message.error.code) ? message.error.code : -32_000;
      const rpcMessage =
        typeof message.error.message === "string" ? message.error.message : "未知 RPC 错误";
      pending.reject(new CodexRpcError(code, rpcMessage));
      return;
    }
    pending.resolve(message.result);
  }

  #write(message) {
    if (this.#closed || this.#child.stdin.destroyed || !this.#child.stdin.writable) {
      return Promise.reject(new Error("Codex App Server 输入流不可用"));
    }
    return new Promise((resolveWrite, rejectWrite) => {
      this.#child.stdin.write(`${JSON.stringify(message)}\n`, (error) => {
        if (error) rejectWrite(error);
        else resolveWrite();
      });
    });
  }

  #failAll(error) {
    for (const pending of this.#pending.values()) pending.reject(error);
    this.#pending.clear();
  }
}
