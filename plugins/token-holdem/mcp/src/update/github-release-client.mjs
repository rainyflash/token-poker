import { UPDATE_REPOSITORY } from "./manifest.mjs";
import {
  ensureSuccessfulResponse,
  fetchTrusted,
  readResponseBytes,
} from "./trusted-fetch.mjs";

const UPDATE_DISCOVERY_URL =
  `https://github.com/${UPDATE_REPOSITORY}/releases/latest/download/latest.json`;
const MAX_MANIFEST_BYTES = 256 * 1024;

export class GitHubReleaseClient {
  #fetch;

  constructor({ fetchImpl = globalThis.fetch } = {}) {
    if (typeof fetchImpl !== "function") throw new Error("A Fetch implementation is required");
    this.#fetch = fetchImpl;
  }

  async fetchLatestManifest() {
    const response = await fetchTrusted(UPDATE_DISCOVERY_URL, this.#fetch);
    ensureSuccessfulResponse(response, "update manifest");
    const bytes = await readResponseBytes(response, MAX_MANIFEST_BYTES);
    try {
      return JSON.parse(bytes.toString("utf8"));
    } catch {
      throw new Error("The update manifest is not valid JSON");
    }
  }
}
