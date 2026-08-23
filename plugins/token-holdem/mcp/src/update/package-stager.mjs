import { createHash, randomBytes } from "node:crypto";
import { createReadStream } from "node:fs";
import {
  copyFile,
  mkdir,
  open,
  rename,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { MAX_UPDATE_BYTES, UPDATE_REPOSITORY } from "./manifest.mjs";
import { ensureSuccessfulResponse, fetchTrusted } from "./trusted-fetch.mjs";

export class FileSystemPackageStager {
  #pluginRoot;
  #stateRoot;
  #fetch;

  constructor({ pluginRoot, stateRoot = defaultUpdateStateRoot(), fetchImpl = globalThis.fetch }) {
    if (typeof fetchImpl !== "function") throw new Error("A Fetch implementation is required");
    this.#pluginRoot = resolve(pluginRoot);
    this.#stateRoot = resolve(stateRoot);
    this.#fetch = fetchImpl;
  }

  async stage(release) {
    const stageDirectory = resolve(this.#stateRoot, `v${release.version}`);
    assertDirectChild(this.#stateRoot, stageDirectory);
    await mkdir(stageDirectory, { recursive: true });
    const archivePath = join(stageDirectory, release.artifact.name);
    const helperPath = join(stageDirectory, "apply-update.ps1");
    const manifestPath = join(stageDirectory, "latest.json");
    const resultPath = join(stageDirectory, "update-result.json");
    const logPath = join(stageDirectory, "install.log");

    if (!(await fileMatches(archivePath, release.artifact))) {
      await downloadVerifiedArtifact({
        artifact: release.artifact,
        destination: archivePath,
        fetchImpl: this.#fetch,
      });
    }
    await Promise.all([
      rm(resultPath, { force: true }),
      rm(logPath, { force: true }),
    ]);
    await copyFile(join(this.#pluginRoot, "scripts", "apply-update.ps1"), helperPath);
    await writeFile(
      manifestPath,
      `${JSON.stringify(serializeRelease(release), null, 2)}\n`,
      "utf8",
    );
    return Object.freeze({
      stageDirectory,
      archivePath,
      helperPath,
      resultPath,
      logPath,
    });
  }
}

export async function downloadVerifiedArtifact({ artifact, destination, fetchImpl }) {
  const resolvedDestination = resolve(destination);
  const partialPath = `${resolvedDestination}.${randomBytes(8).toString("hex")}.partial`;
  const response = await fetchTrusted(artifact.downloadUrl, fetchImpl);
  ensureSuccessfulResponse(response, "update package");
  const advertisedLength = response.headers.get("content-length");
  if (advertisedLength !== null && Number(advertisedLength) !== artifact.bytes) {
    throw new Error("The update server reported an unexpected package size");
  }
  if (response.body === null) throw new Error("The update package response has no body");

  const digest = createHash("sha256");
  let receivedBytes = 0;
  const file = await open(partialPath, "wx");
  try {
    const source = Readable.fromWeb(response.body);
    const limiter = async function* (chunks) {
      for await (const rawChunk of chunks) {
        const chunk = Buffer.from(rawChunk);
        receivedBytes += chunk.byteLength;
        if (receivedBytes > artifact.bytes || receivedBytes > MAX_UPDATE_BYTES) {
          throw new Error("The update package exceeded its declared size");
        }
        digest.update(chunk);
        yield chunk;
      }
    };
    await pipeline(source, limiter, file.createWriteStream());
    if (receivedBytes !== artifact.bytes) {
      throw new Error("The downloaded update package is incomplete");
    }
    if (digest.digest("hex") !== artifact.sha256) {
      throw new Error("The downloaded update package failed SHA-256 verification");
    }
    await rm(resolvedDestination, { force: true });
    await rename(partialPath, resolvedDestination);
  } catch (error) {
    await file.close().catch(() => undefined);
    await rm(partialPath, { force: true }).catch(() => undefined);
    throw error;
  }
  return resolvedDestination;
}

async function fileMatches(path, artifact) {
  try {
    const fileStat = await stat(path);
    if (!fileStat.isFile() || fileStat.size !== artifact.bytes) return false;
    return (await hashFile(path)) === artifact.sha256;
  } catch {
    return false;
  }
}

async function hashFile(path) {
  const digest = createHash("sha256");
  await pipeline(createReadStream(path), digest);
  return digest.digest("hex");
}

function defaultUpdateStateRoot() {
  const localAppData = process.env.LOCALAPPDATA;
  return join(
    typeof localAppData === "string" && localAppData.length > 0 ? localAppData : tmpdir(),
    "TokenHoldem",
    "updates",
  );
}

function assertDirectChild(parent, child) {
  const expectedParent = resolve(parent);
  if (resolve(child, "..") !== expectedParent) {
    throw new Error("The update staging path escaped its state directory");
  }
}

function serializeRelease(release) {
  return {
    schema_version: 1,
    version: release.version,
    tag: release.tag,
    repository: UPDATE_REPOSITORY,
    release_url: release.releaseUrl,
    artifact: {
      target: release.artifact.target,
      name: release.artifact.name,
      bytes: release.artifact.bytes,
      sha256: release.artifact.sha256,
      download_url: release.artifact.downloadUrl,
    },
  };
}
