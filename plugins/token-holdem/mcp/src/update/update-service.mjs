import { parseUpdateManifest } from "./manifest.mjs";

export class UpdateService {
  #currentVersion;
  #releaseClient;
  #packageStager;
  #installerLauncher;
  #latest = null;
  #prepared = null;
  #operation = Promise.resolve();
  #snapshot;

  constructor({ currentVersion, releaseClient, packageStager, installerLauncher }) {
    this.#currentVersion = currentVersion;
    this.#releaseClient = releaseClient;
    this.#packageStager = packageStager;
    this.#installerLauncher = installerLauncher;
    this.#snapshot = freezeSnapshot({
      phase: "idle",
      current_version: currentVersion,
      latest_version: null,
      release_url: null,
      artifact_bytes: null,
      downloaded_bytes: 0,
      sha256_verified: false,
      error: null,
    });
  }

  get snapshot() {
    return this.#snapshot;
  }

  async check() {
    return this.#runExclusive(async () => {
      this.#set({ phase: "checking", error: null });
      try {
        const manifest = parseUpdateManifest(
          await this.#releaseClient.fetchLatestManifest(),
          this.#currentVersion,
        );
        this.#latest = manifest;
        this.#prepared = null;
        return this.#set({
          phase: manifest.updateAvailable ? "available" : "current",
          latest_version: manifest.version,
          release_url: manifest.releaseUrl,
          artifact_bytes: manifest.artifact.bytes,
          downloaded_bytes: 0,
          sha256_verified: false,
          error: null,
        });
      } catch (error) {
        return this.#fail(error);
      }
    });
  }

  async prepare() {
    return this.#runExclusive(async () => {
      if (this.#latest === null || !this.#latest.updateAvailable) {
        return this.#fail(new Error("No newer stable update is available"));
      }
      this.#set({ phase: "downloading", downloaded_bytes: 0, error: null });
      try {
        this.#prepared = await this.#packageStager.stage(this.#latest);
        return this.#set({
          phase: "ready",
          downloaded_bytes: this.#latest.artifact.bytes,
          sha256_verified: true,
          error: null,
        });
      } catch (error) {
        this.#prepared = null;
        return this.#fail(error);
      }
    });
  }

  async install() {
    return this.#runExclusive(async () => {
      if (this.#latest === null || this.#prepared === null) {
        return this.#fail(new Error("The update package has not been downloaded and verified"));
      }
      this.#set({ phase: "installing", error: null });
      try {
        await this.#installerLauncher.launch({
          release: this.#latest,
          prepared: this.#prepared,
          parentProcessId: process.pid,
        });
        return this.#set({ phase: "restart_required", error: null });
      } catch (error) {
        return this.#fail(error);
      }
    });
  }

  async #runExclusive(operation) {
    const result = this.#operation.then(operation, operation);
    this.#operation = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  #fail(error) {
    return this.#set({
      phase: "error",
      error: error instanceof Error ? error.message : "The update operation failed",
    });
  }

  #set(patch) {
    this.#snapshot = freezeSnapshot({ ...this.#snapshot, ...patch });
    return this.#snapshot;
  }
}

function freezeSnapshot(snapshot) {
  return Object.freeze({ ...snapshot });
}
