import { createRequire } from "node:module";
import { copyFile, mkdir, stat } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const require = createRequire(import.meta.url);
const mcpRoot = dirname(fileURLToPath(import.meta.url));
const pluginRoot = resolve(mcpRoot, "..");
const projectRoot = resolve(pluginRoot, "..", "..");
const executableName = process.platform === "win32" ? "token-holdem-sidecar.exe" : "token-holdem-sidecar";
const runtimeExecutableName =
  process.platform === "win32" ? "token-holdem-runtime.exe" : "token-holdem-runtime";

await Promise.all([
  mkdir(join(pluginRoot, "bin"), { recursive: true }),
  mkdir(join(pluginRoot, "config"), { recursive: true }),
  mkdir(join(pluginRoot, "scripts"), { recursive: true }),
  mkdir(join(pluginRoot, "ui"), { recursive: true }),
  mkdir(join(mcpRoot, "vendor"), { recursive: true }),
]);

await build({
  entryPoints: [join(mcpRoot, "src", "index.mjs")],
  outfile: join(mcpRoot, "server.bundle.mjs"),
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node20",
  sourcemap: false,
  legalComments: "eof",
});

const sidecarSource = await newestExisting([
  join(projectRoot, "target", "release", executableName),
  join(projectRoot, "target", "debug", executableName),
]);
const runtimeSource = await newestExisting([
  join(projectRoot, "target", "release", runtimeExecutableName),
  join(projectRoot, "target", "debug", runtimeExecutableName),
]);

await Promise.all([
  copyFile(sidecarSource, join(pluginRoot, "bin", executableName)),
  copyFile(runtimeSource, join(pluginRoot, "bin", runtimeExecutableName)),
  copyFile(
    join(projectRoot, "config", "community-nodes.json"),
    join(pluginRoot, "config", "community-nodes.json"),
  ),
  copyFile(
    join(projectRoot, "scripts", "apply-update.ps1"),
    join(pluginRoot, "scripts", "apply-update.ps1"),
  ),
  copyFile(
    join(projectRoot, "ui", "dist", "token-holdem.js"),
    join(pluginRoot, "ui", "token-holdem.js"),
  ),
  copyFile(
    join(projectRoot, "ui", "dist", "token-holdem.css"),
    join(pluginRoot, "ui", "token-holdem.css"),
  ),
  copyFile(
    require.resolve("@modelcontextprotocol/ext-apps/app-with-deps"),
    join(mcpRoot, "vendor", "ext-apps-app-with-deps.js"),
  ),
  copyFile(join(projectRoot, "LICENSE-MIT"), join(pluginRoot, "LICENSE-MIT")),
  copyFile(join(projectRoot, "LICENSE-APACHE"), join(pluginRoot, "LICENSE-APACHE")),
]);

process.stdout.write(`Token Poker 官方插件已构建：${pluginRoot}\n`);

async function newestExisting(candidates) {
  const existing = [];
  for (const candidate of candidates) {
    try {
      existing.push({ candidate, modifiedAt: (await stat(candidate)).mtimeMs });
    } catch {
      continue;
    }
  }
  existing.sort((left, right) => right.modifiedAt - left.modifiedAt);
  if (existing.length > 0) return existing[0].candidate;
  throw new Error(`缺少已编译 sidecar：${candidates.join("；")}`);
}
