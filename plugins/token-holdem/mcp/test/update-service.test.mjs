import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { downloadVerifiedArtifact } from "../src/update/package-stager.mjs";
import { UpdateService } from "../src/update/update-service.mjs";

const RELEASE = Object.freeze({
  version: "0.4.1",
  tag: "v0.4.1",
  releaseUrl: "https://github.com/rainyflash/token-poker/releases/tag/v0.4.1",
  updateAvailable: true,
  artifact: Object.freeze({
    target: "windows-x64",
    name: "token-poker-plugin-v0.4.1-windows-x64.zip",
    bytes: 4,
    sha256: createHash("sha256").update("test").digest("hex"),
    downloadUrl:
      "https://github.com/rainyflash/token-poker/releases/download/v0.4.1/token-poker-plugin-v0.4.1-windows-x64.zip",
  }),
});

test("moves the update facade through check, verify, and detached install", async () => {
  const calls = [];
  const service = new UpdateService({
    currentVersion: "0.4.0",
    releaseClient: { fetchLatestManifest: async () => manifestForRelease(RELEASE) },
    packageStager: {
      stage: async (release) => {
        calls.push(["stage", release.version]);
        return { archivePath: "archive.zip", helperPath: "apply-update.ps1" };
      },
    },
    installerLauncher: {
      launch: async ({ release, parentProcessId }) => {
        calls.push(["launch", release.version, Number.isSafeInteger(parentProcessId)]);
      },
    },
  });

  assert.equal((await service.check()).phase, "available");
  assert.equal((await service.prepare()).phase, "ready");
  assert.equal((await service.install()).phase, "restart_required");
  assert.deepEqual(calls, [
    ["stage", "0.4.1"],
    ["launch", "0.4.1", true],
  ]);
});

test("fails closed when install is requested before verification", async () => {
  const service = new UpdateService({
    currentVersion: "0.4.0",
    releaseClient: { fetchLatestManifest: async () => manifestForRelease(RELEASE) },
    packageStager: { stage: async () => assert.fail("stager should not run") },
    installerLauncher: { launch: async () => assert.fail("launcher should not run") },
  });
  assert.equal((await service.install()).phase, "error");
  assert.match(service.snapshot.error, /not been downloaded and verified/u);
});

test("downloads exactly the declared bytes and rejects a digest mismatch", async (context) => {
  const directory = await mkdtemp(join(tmpdir(), "token-poker-update-test-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const destination = join(directory, RELEASE.artifact.name);
  const fetchImpl = async () =>
    new Response(Buffer.from("test"), {
      status: 200,
      headers: { "content-length": "4" },
    });

  await downloadVerifiedArtifact({
    artifact: RELEASE.artifact,
    destination,
    fetchImpl,
  });
  assert.equal((await readFile(destination, "utf8")), "test");

  await assert.rejects(
    downloadVerifiedArtifact({
      artifact: { ...RELEASE.artifact, sha256: "0".repeat(64) },
      destination: join(directory, "mismatch.zip"),
      fetchImpl,
    }),
    /SHA-256/u,
  );
});

test("rejects an update redirect outside the GitHub asset boundary", async (context) => {
  const directory = await mkdtemp(join(tmpdir(), "token-poker-redirect-test-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const fetchImpl = async () =>
    new Response(null, {
      status: 302,
      headers: { location: "https://example.com/attacker.zip" },
    });
  await assert.rejects(
    downloadVerifiedArtifact({
      artifact: RELEASE.artifact,
      destination: join(directory, RELEASE.artifact.name),
      fetchImpl,
    }),
    /untrusted origin/u,
  );
});

test("rejects a package whose HTTP length differs from the release manifest", async (context) => {
  const directory = await mkdtemp(join(tmpdir(), "token-poker-length-test-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const fetchImpl = async () =>
    new Response(Buffer.from("short"), {
      status: 200,
      headers: { "content-length": "5" },
    });
  await assert.rejects(
    downloadVerifiedArtifact({
      artifact: RELEASE.artifact,
      destination: join(directory, RELEASE.artifact.name),
      fetchImpl,
    }),
    /unexpected package size/u,
  );
});

function manifestForRelease(release) {
  return {
    schema_version: 1,
    channel: "stable",
    version: release.version,
    tag: release.tag,
    repository: "rainyflash/token-poker",
    release_url: release.releaseUrl,
    artifacts: [
      {
        target: release.artifact.target,
        name: release.artifact.name,
        bytes: release.artifact.bytes,
        sha256: release.artifact.sha256,
        download_url: release.artifact.downloadUrl,
      },
    ],
  };
}
