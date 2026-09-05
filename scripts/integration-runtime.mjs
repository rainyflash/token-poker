import { resolve } from "node:path";

export function integrationSidecarPath(projectRoot, environment = process.env) {
  const configuredPath = environment.TOKEN_HOLDEM_SIDECAR_PATH?.trim();
  return configuredPath
    ? resolve(projectRoot, configuredPath)
    : resolve(projectRoot, "target", "debug", "token-holdem-sidecar.exe");
}
