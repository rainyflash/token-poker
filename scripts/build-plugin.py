from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path, PurePosixPath


if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8")


PROJECT_ROOT = Path(__file__).resolve().parent.parent
DIST_DIR = PROJECT_ROOT / "dist"
PLUGIN_DIR = PROJECT_ROOT / "plugins" / "token-holdem"
MARKETPLACE_MANIFEST = PROJECT_ROOT / ".agents" / "plugins" / "marketplace.json"
RELEASE_FILES_MANIFEST = PLUGIN_DIR / "release-files.json"
DEFAULT_REPOSITORY = "rainyflash/token-poker"
TARGET = "windows-x64"
SEMVER_PATTERN = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?")
REPOSITORY_PATTERN = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")


def run(*command: str, working_directory: Path = PROJECT_ROOT) -> None:
    print(f"Running: {' '.join(command)}", flush=True)
    subprocess.run(command, cwd=working_directory, check=True)


def find_command(name: str) -> str:
    candidates = [name]
    if sys.platform == "win32":
        candidates = [f"{name}.exe", f"{name}.cmd", name]
    for candidate in candidates:
        path = shutil.which(candidate)
        if path is not None:
            return path
    raise RuntimeError(f"Required build command is unavailable: {name}")


def read_json(path: Path) -> object:
    return json.loads(path.read_text(encoding="utf-8"))


def read_version() -> str:
    cargo_manifest = (PROJECT_ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(
        r'(?ms)^\[workspace\.package\]\s*.*?^version\s*=\s*"(?P<version>[^"]+)"',
        cargo_manifest,
    )
    if match is None:
        raise RuntimeError("Could not read the workspace version from Cargo.toml")

    version = match.group("version")
    if SEMVER_PATTERN.fullmatch(version) is None:
        raise RuntimeError(f"Invalid workspace semantic version: {version}")

    manifests = {
        "Codex plugin": PLUGIN_DIR / ".codex-plugin" / "plugin.json",
        "MCP package": PLUGIN_DIR / "mcp" / "package.json",
        "MCP lockfile": PLUGIN_DIR / "mcp" / "package-lock.json",
        "UI package": PROJECT_ROOT / "ui" / "package.json",
        "UI lockfile": PROJECT_ROOT / "ui" / "package-lock.json",
    }
    for label, path in manifests.items():
        manifest = read_json(path)
        if not isinstance(manifest, dict) or manifest.get("version") != version:
            actual = manifest.get("version") if isinstance(manifest, dict) else None
            raise RuntimeError(
                f"{label} version {actual!r} does not match workspace version {version}"
            )
    return version


def read_release_files() -> list[str]:
    manifest = read_json(RELEASE_FILES_MANIFEST)
    if not isinstance(manifest, dict) or manifest.get("schema_version") != 1:
        raise RuntimeError("Unsupported release-files manifest schema")
    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        raise RuntimeError("The release-files manifest must contain a non-empty files array")

    normalized: list[str] = []
    seen: set[str] = set()
    for raw_path in files:
        if not isinstance(raw_path, str) or not raw_path:
            raise RuntimeError("The release-files manifest contains an invalid path")
        path = PurePosixPath(raw_path)
        if path.is_absolute() or ".." in path.parts or "\\" in raw_path:
            raise RuntimeError(f"Release payload path escapes the plugin root: {raw_path}")
        relative_path = path.as_posix()
        if relative_path in seen:
            raise RuntimeError(f"Duplicate release payload path: {relative_path}")
        seen.add(relative_path)
        normalized.append(relative_path)
    return normalized


def copy_file(source: Path, destination: Path) -> None:
    if not source.is_file():
        raise RuntimeError(f"Required release file is missing: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def build() -> None:
    cargo = find_command("cargo")
    npm = find_command("npm")
    run(cargo, "build", "--locked", "--release", "-p", "token-holdem-sidecar")

    executable_suffix = ".exe" if sys.platform == "win32" else ""
    for executable_name in ("token-holdem-sidecar", "token-holdem-runtime"):
        copy_file(
            PROJECT_ROOT
            / "target"
            / "release"
            / f"{executable_name}{executable_suffix}",
            PLUGIN_DIR / "bin" / f"{executable_name}{executable_suffix}",
        )

    ui_directory = PROJECT_ROOT / "ui"
    run(npm, "ci", working_directory=ui_directory)
    run(npm, "run", "build", working_directory=ui_directory)
    run(npm, "run", "lint", working_directory=ui_directory)
    run(npm, "test", working_directory=ui_directory)

    mcp_directory = PLUGIN_DIR / "mcp"
    run(npm, "ci", working_directory=mcp_directory)
    run(npm, "run", "build", working_directory=mcp_directory)
    run(npm, "test", working_directory=mcp_directory)


def write_package_manifest(staging_directory: Path, version: str) -> None:
    files: list[dict[str, object]] = []
    for path in sorted(staging_directory.rglob("*")):
        if not path.is_file() or path.name == "manifest.json":
            continue
        content = path.read_bytes()
        files.append(
            {
                "path": path.relative_to(staging_directory).as_posix(),
                "bytes": len(content),
                "sha256": hashlib.sha256(content).hexdigest(),
            }
        )

    manifest = {
        "schema_version": 1,
        "name": "token-poker-plugin",
        "version": version,
        "target": TARGET,
        "unsigned": True,
        "runtime": {
            "host": "codex",
            "node": "codex-managed",
            "system_node_required": False,
            "private_node_bundled": False,
            "codex_app_server": {
                "source": "copied-from-installed-codex-desktop-during-install",
                "required_method": "account/usage/read",
                "private_binary_bundled": False,
                "installed_copy_required": True,
            },
        },
        "files": files,
    }
    (staging_directory / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n",
        encoding="utf-8",
        newline="\n",
    )


def package(version: str) -> tuple[Path, Path, str]:
    package_name = f"token-poker-plugin-v{version}-{TARGET}"
    staging_directory = (DIST_DIR / package_name).resolve()
    archive_path = (DIST_DIR / f"{package_name}.zip").resolve()
    checksum_path = Path(f"{archive_path}.sha256")
    dist_root = DIST_DIR.resolve()
    if staging_directory.parent != dist_root or archive_path.parent != dist_root:
        raise RuntimeError("Refusing to create or clean release files outside dist")

    DIST_DIR.mkdir(parents=True, exist_ok=True)
    if staging_directory.exists():
        shutil.rmtree(staging_directory)
    for old_file in (archive_path, checksum_path):
        if old_file.exists():
            old_file.unlink()

    copy_file(
        MARKETPLACE_MANIFEST,
        staging_directory / ".agents" / "plugins" / "marketplace.json",
    )
    for relative_file in (
        ".mcp.json",
        ".codex-plugin/plugin.json",
        "release-files.json",
    ):
        copy_file(
            PLUGIN_DIR / relative_file,
            staging_directory / "plugins" / "token-holdem" / relative_file,
        )
    for relative_file in read_release_files():
        copy_file(
            PLUGIN_DIR / relative_file,
            staging_directory / "plugins" / "token-holdem" / relative_file,
        )

    copy_file(
        PROJECT_ROOT / "scripts" / "install-token-poker.cmd",
        staging_directory / "Install Token Poker.cmd",
    )
    copy_file(
        PROJECT_ROOT / "scripts" / "install-plugin.ps1",
        staging_directory / "install-token-poker.ps1",
    )
    copy_file(
        PROJECT_ROOT / "scripts" / "codex-runtime.ps1",
        staging_directory / "codex-runtime.ps1",
    )
    copy_file(PROJECT_ROOT / "docs" / "PLUGIN-DISTRIBUTION.md", staging_directory / "README.md")
    copy_file(PROJECT_ROOT / "SECURITY.md", staging_directory / "SECURITY.md")
    copy_file(PROJECT_ROOT / "LICENSE-MIT", staging_directory / "LICENSE-MIT")
    copy_file(PROJECT_ROOT / "LICENSE-APACHE", staging_directory / "LICENSE-APACHE")
    write_package_manifest(staging_directory, version)

    stable_timestamp = (1980, 1, 1, 0, 0, 0)
    with zipfile.ZipFile(
        archive_path,
        "w",
        compression=zipfile.ZIP_DEFLATED,
        compresslevel=9,
    ) as archive:
        for path in sorted(staging_directory.rglob("*")):
            if not path.is_file():
                continue
            archive_name = f"{package_name}/{path.relative_to(staging_directory).as_posix()}"
            info = zipfile.ZipInfo(archive_name, date_time=stable_timestamp)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(
                info,
                path.read_bytes(),
                compress_type=zipfile.ZIP_DEFLATED,
                compresslevel=9,
            )

    with zipfile.ZipFile(archive_path, "r") as archive:
        bad_file = archive.testzip()
        if bad_file is not None:
            raise RuntimeError(f"Release ZIP failed integrity validation: {bad_file}")

    digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    checksum_path.write_text(
        f"{digest}  {archive_path.name}\n",
        encoding="ascii",
        newline="\n",
    )
    return archive_path, checksum_path, digest


def write_update_manifest(
    *,
    version: str,
    repository: str,
    archive_path: Path,
    digest: str,
) -> Path:
    if REPOSITORY_PATTERN.fullmatch(repository) is None:
        raise RuntimeError(f"Invalid GitHub repository identifier: {repository}")
    if re.fullmatch(r"[0-9a-f]{64}", digest) is None:
        raise RuntimeError("Invalid SHA-256 digest for update manifest")

    tag = f"v{version}"
    download_url = (
        f"https://github.com/{repository}/releases/download/{tag}/{archive_path.name}"
    )
    manifest = {
        "schema_version": 1,
        "channel": "stable",
        "version": version,
        "tag": tag,
        "repository": repository,
        "release_url": f"https://github.com/{repository}/releases/tag/{tag}",
        "artifacts": [
            {
                "target": TARGET,
                "name": archive_path.name,
                "bytes": archive_path.stat().st_size,
                "sha256": digest,
                "download_url": download_url,
            }
        ],
    }
    manifest_path = DIST_DIR / "latest.json"
    manifest_path.write_text(
        json.dumps(manifest, indent=2) + "\n",
        encoding="utf-8",
        newline="\n",
    )

    round_trip = read_json(manifest_path)
    if round_trip != manifest:
        raise RuntimeError("Update manifest failed round-trip validation")
    return manifest_path


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Build the unsigned Token Poker Codex plugin release"
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="Reuse existing build outputs and only recreate release artifacts",
    )
    parser.add_argument(
        "--repository",
        default=os.environ.get("GITHUB_REPOSITORY", DEFAULT_REPOSITORY),
        help="GitHub owner/repository used in latest.json",
    )
    arguments = parser.parse_args()

    version = read_version()
    if not arguments.skip_build:
        build()
    archive_path, checksum_path, digest = package(version)
    update_manifest_path = write_update_manifest(
        version=version,
        repository=arguments.repository,
        archive_path=archive_path,
        digest=digest,
    )

    print(f"Plugin package: {archive_path}")
    print(f"Checksum: {checksum_path}")
    print(f"Update manifest: {update_manifest_path}")
    print("This release is unsigned; SHA-256 verifies byte integrity only.")


if __name__ == "__main__":
    main()
