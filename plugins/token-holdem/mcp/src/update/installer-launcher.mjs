import { spawn } from "node:child_process";
import { access } from "node:fs/promises";
import { join } from "node:path";

export class DetachedInstallerLauncher {
  async launch({ release, prepared, parentProcessId }) {
    const powershell = await resolvePowerShell();
    const child = spawn(
      powershell,
      [
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        prepared.helperPath,
        "-ArchivePath",
        prepared.archivePath,
        "-ExpectedVersion",
        release.version,
        "-ExpectedSha256",
        release.artifact.sha256,
        "-ExpectedBytes",
        String(release.artifact.bytes),
        "-ParentProcessId",
        String(parentProcessId),
      ],
      {
        detached: true,
        stdio: "ignore",
        windowsHide: true,
      },
    );
    await new Promise((resolveSpawn, rejectSpawn) => {
      child.once("spawn", resolveSpawn);
      child.once("error", rejectSpawn);
    });
    child.unref();
  }
}

async function resolvePowerShell() {
  const systemRoot = process.env.SystemRoot;
  if (typeof systemRoot === "string" && systemRoot.length > 0) {
    const candidate = join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
    try {
      await access(candidate);
      return candidate;
    } catch {
      // The command lookup fallback remains necessary on non-standard Windows images.
    }
  }
  return "powershell.exe";
}
