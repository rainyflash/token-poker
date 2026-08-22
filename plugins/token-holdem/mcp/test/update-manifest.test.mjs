import assert from "node:assert/strict";
import test from "node:test";
import {
  compareSemanticVersions,
  parseUpdateManifest,
} from "../src/update/manifest.mjs";

const VALID_MANIFEST = Object.freeze({
  schema_version: 1,
  channel: "stable",
  version: "0.4.1",
  tag: "v0.4.1",
  repository: "rainyflash/token-poker",
  release_url: "https://github.com/rainyflash/token-poker/releases/tag/v0.4.1",
  artifacts: [
    {
      target: "windows-x64",
      name: "token-poker-plugin-v0.4.1-windows-x64.zip",
      bytes: 8_000_000,
      sha256: "a".repeat(64),
      download_url:
        "https://github.com/rainyflash/token-poker/releases/download/v0.4.1/token-poker-plugin-v0.4.1-windows-x64.zip",
    },
  ],
});

test("accepts the exact stable GitHub release contract", () => {
  const update = parseUpdateManifest(VALID_MANIFEST, "0.4.0");
  assert.equal(update.updateAvailable, true);
  assert.equal(update.version, "0.4.1");
  assert.equal(update.artifact.bytes, 8_000_000);
});

test("does not offer the installed or an older release", () => {
  assert.equal(parseUpdateManifest(VALID_MANIFEST, "0.4.1").updateAvailable, false);
  assert.equal(parseUpdateManifest(VALID_MANIFEST, "1.0.0").updateAvailable, false);
});

test("rejects repository, URL, target, size, and version substitutions", () => {
  const mutations = [
    { ...VALID_MANIFEST, repository: "attacker/token-poker" },
    { ...VALID_MANIFEST, release_url: "https://example.com/v0.4.1" },
    {
      ...VALID_MANIFEST,
      artifacts: [{ ...VALID_MANIFEST.artifacts[0], target: "linux-x64" }],
    },
    {
      ...VALID_MANIFEST,
      artifacts: [{ ...VALID_MANIFEST.artifacts[0], bytes: 600 * 1024 * 1024 }],
    },
    { ...VALID_MANIFEST, version: "0.4.1-beta.1" },
  ];
  for (const manifest of mutations) {
    assert.throws(() => parseUpdateManifest(manifest, "0.4.0"));
  }
});

test("compares stable versions numerically", () => {
  assert.equal(compareSemanticVersions("0.10.0", "0.9.9"), 1);
  assert.equal(compareSemanticVersions("1.0.0", "1.0.0"), 0);
  assert.equal(compareSemanticVersions("1.0.0", "1.0.1"), -1);
  assert.equal(
    compareSemanticVersions("9007199254740993.0.0", "9007199254740992.999.999"),
    1,
  );
});
