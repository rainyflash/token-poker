import { GitHubReleaseClient } from "./github-release-client.mjs";
import { InstallerLauncher } from "./installer-launcher.mjs";
import { FileSystemPackageStager } from "./package-stager.mjs";
import { UpdateService } from "./update-service.mjs";

export function createUpdateService({ currentVersion, pluginRoot }) {
  return new UpdateService({
    currentVersion,
    releaseClient: new GitHubReleaseClient(),
    packageStager: new FileSystemPackageStager({ pluginRoot }),
    installerLauncher: new InstallerLauncher(),
  });
}
