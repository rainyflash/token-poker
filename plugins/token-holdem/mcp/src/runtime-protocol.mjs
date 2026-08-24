import { createHash } from "node:crypto";
import { homedir, userInfo } from "node:os";

export const RUNTIME_PROTOCOL_VERSION = 6;
const PIPE_PREFIX = String.raw`\\.\pipe\token-holdem-runtime-v6-`;

export function runtimePipeName() {
  const override = process.env.TOKEN_HOLDEM_RUNTIME_PIPE;
  if (typeof override === "string" && override.length > 0) {
    validatePipeName(override);
    return override;
  }
  const identity = `${homedir()}\0${userInfo().username}`.toLocaleLowerCase("en-US");
  const suffix = createHash("sha256").update(identity, "utf8").digest("hex").slice(0, 24);
  return `${PIPE_PREFIX}${suffix}`;
}

export function runtimeAttachFrame() {
  return Object.freeze({
    type: "runtime_attach",
    protocol_version: RUNTIME_PROTOCOL_VERSION,
  });
}

export function parseRuntimeFrame(line) {
  let value;
  try {
    value = JSON.parse(line);
  } catch {
    throw new Error("共享运行时输出了无效 JSON");
  }
  if (!isRecord(value) || typeof value.type !== "string") {
    throw new Error("共享运行时帧缺少类型");
  }
  switch (value.type) {
    case "runtime_attached":
      if (
        value.protocol_version !== RUNTIME_PROTOCOL_VERSION ||
        typeof value.runtime_id !== "string" ||
        value.runtime_id.length === 0 ||
        !(value.worker_pid === null || isNonNegativeSafeInteger(value.worker_pid)) ||
        !isPositiveSafeInteger(value.generation) ||
        !isNonNegativeSafeInteger(value.latest_sequence) ||
        !isNonNegativeSafeInteger(value.earliest_sequence) ||
        typeof value.history_truncated !== "boolean"
      ) {
        throw new Error("共享运行时附着帧无效");
      }
      return value;
    case "runtime_event":
      if (
        !isPositiveSafeInteger(value.generation) ||
        !isPositiveSafeInteger(value.sequence) ||
        !isRecord(value.event) ||
        typeof value.event.type !== "string"
      ) {
        throw new Error("共享运行时事件帧无效");
      }
      return value;
    case "runtime_replay_complete":
      if (
        !isPositiveSafeInteger(value.generation) ||
        !isNonNegativeSafeInteger(value.latest_sequence)
      ) {
        throw new Error("共享运行时回放完成帧无效");
      }
      return value;
    case "runtime_error":
      if (typeof value.code !== "string" || typeof value.message !== "string") {
        throw new Error("共享运行时错误帧无效");
      }
      return value;
    default:
      throw new Error(`未知共享运行时帧：${value.type}`);
  }
}

function validatePipeName(value) {
  if (!value.startsWith(PIPE_PREFIX)) {
    throw new Error("共享运行时命名管道前缀无效");
  }
  const suffix = value.slice(PIPE_PREFIX.length);
  if (!/^[0-9a-f]{12,64}$/u.test(suffix)) {
    throw new Error("共享运行时命名管道后缀必须是十六进制文本");
  }
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonNegativeSafeInteger(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

function isPositiveSafeInteger(value) {
  return isNonNegativeSafeInteger(value) && value > 0;
}
