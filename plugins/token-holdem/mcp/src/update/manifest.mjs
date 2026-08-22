import { z } from "zod";

export const UPDATE_REPOSITORY = "rainyflash/token-poker";
export const UPDATE_TARGET = "windows-x64";
export const MAX_UPDATE_BYTES = 512 * 1024 * 1024;

const stableVersionSchema = z
  .string()
  .regex(/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/u);

const artifactSchema = z
  .object({
    target: z.literal(UPDATE_TARGET),
    name: z.string().min(1),
    bytes: z.number().int().positive().max(MAX_UPDATE_BYTES),
    sha256: z.string().regex(/^[0-9a-f]{64}$/u),
    download_url: z.string().url(),
  })
  .strict();

const manifestSchema = z
  .object({
    schema_version: z.literal(1),
    channel: z.literal("stable"),
    version: stableVersionSchema,
    tag: z.string().min(1),
    repository: z.literal(UPDATE_REPOSITORY),
    release_url: z.string().url(),
    artifacts: z.tuple([artifactSchema]),
  })
  .strict();

export function parseUpdateManifest(value, currentVersion) {
  const current = parseStableVersion(currentVersion);
  const manifest = manifestSchema.parse(value);
  const latest = parseStableVersion(manifest.version);
  const expectedTag = `v${manifest.version}`;
  const expectedName = `token-poker-plugin-${expectedTag}-${UPDATE_TARGET}.zip`;
  const expectedReleaseUrl = `https://github.com/${UPDATE_REPOSITORY}/releases/tag/${expectedTag}`;
  const expectedDownloadUrl =
    `https://github.com/${UPDATE_REPOSITORY}/releases/download/${expectedTag}/${expectedName}`;
  const artifact = manifest.artifacts[0];

  if (manifest.tag !== expectedTag) {
    throw new Error("The update manifest tag does not match its version");
  }
  if (manifest.release_url !== expectedReleaseUrl) {
    throw new Error("The update manifest release URL is not trusted");
  }
  if (artifact.name !== expectedName) {
    throw new Error("The update artifact name does not match its version and target");
  }
  if (artifact.download_url !== expectedDownloadUrl) {
    throw new Error("The update artifact URL is not trusted");
  }

  return Object.freeze({
    version: manifest.version,
    tag: manifest.tag,
    releaseUrl: manifest.release_url,
    updateAvailable: compareVersionParts(latest, current) > 0,
    artifact: Object.freeze({
      target: artifact.target,
      name: artifact.name,
      bytes: artifact.bytes,
      sha256: artifact.sha256,
      downloadUrl: artifact.download_url,
    }),
  });
}

export function compareSemanticVersions(left, right) {
  return compareVersionParts(parseStableVersion(left), parseStableVersion(right));
}

function parseStableVersion(value) {
  const parsed = stableVersionSchema.parse(value).split(".").map(BigInt);
  return Object.freeze(parsed);
}

function compareVersionParts(left, right) {
  for (let index = 0; index < 3; index += 1) {
    if (left[index] !== right[index]) return left[index] > right[index] ? 1 : -1;
  }
  return 0;
}
