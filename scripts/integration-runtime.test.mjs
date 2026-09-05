import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import test from "node:test";
import { integrationSidecarPath } from "./integration-runtime.mjs";

const projectRoot = resolve(import.meta.dirname, "..");

test("未指定二进制时保留本地调试默认值", () => {
  assert.equal(integrationSidecarPath(projectRoot, {}),
    join(projectRoot, "target", "debug", "token-holdem-sidecar.exe"));
});

test("指定发布二进制后不回退到调试目录", () => {
  assert.equal(integrationSidecarPath(projectRoot, {
    TOKEN_HOLDEM_SIDECAR_PATH: "target/release/token-holdem-sidecar.exe",
  }), join(projectRoot, "target", "release", "token-holdem-sidecar.exe"));
});

for (const script of [
  "verify-volunteer-network.mjs",
  "verify-p2p-hand.mjs",
  "verify-dynamic-table.mjs",
  "verify-safe-leave.mjs",
  "verify-complete-session.mjs",
]) {
  test(`${script} 必须使用指定路径，不能借用本机已有调试产物`, () => {
    const missingRuntime = join(tmpdir(), `token-poker-missing-${randomUUID()}.exe`);
    const result = spawnSync(process.execPath, [join(import.meta.dirname, script)], {
      cwd: projectRoot,
      env: { ...process.env, TOKEN_HOLDEM_SIDECAR_PATH: missingRuntime },
      encoding: "utf8",
      timeout: 10000,
      windowsHide: true,
    });
    assert.equal(result.error, undefined);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /ENOENT/u);
    assert.ok(result.stderr.includes(basename(missingRuntime)), result.stderr);
  });
}
