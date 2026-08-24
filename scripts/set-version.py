from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


PROJECT_ROOT = Path(__file__).resolve().parent.parent
SEMVER_PATTERN = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")


def parse_version(value: str) -> tuple[int, int, int]:
    match = SEMVER_PATTERN.fullmatch(value)
    if match is None:
        raise ValueError(f"Unsupported stable semantic version: {value}")
    return tuple(int(part) for part in match.groups())


def replace_once(text: str, pattern: re.Pattern[str], replacement: str, label: str) -> str:
    updated, count = pattern.subn(replacement, text, count=1)
    if count != 1:
        raise RuntimeError(f"Could not update {label}")
    return updated


def load_json(path: Path) -> dict[str, object]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise RuntimeError(f"Expected a JSON object: {path}")
    return value


def serialize_json(value: dict[str, object]) -> str:
    return json.dumps(value, indent=2, ensure_ascii=False) + "\n"


def update_json_manifest(path: Path, current: str, target: str) -> str:
    value = load_json(path)
    if value.get("version") != current:
        raise RuntimeError(f"Unexpected version in {path}: {value.get('version')!r}")
    value["version"] = target
    return serialize_json(value)


def update_package_lock(path: Path, current: str, target: str) -> str:
    value = load_json(path)
    packages = value.get("packages")
    if not isinstance(packages, dict) or not isinstance(packages.get(""), dict):
        raise RuntimeError(f"Unsupported npm lockfile shape: {path}")
    root_package = packages[""]
    if value.get("version") != current or root_package.get("version") != current:
        raise RuntimeError(f"Unexpected root package version in {path}")
    value["version"] = target
    root_package["version"] = target
    return serialize_json(value)


def prepare_updates(target: str) -> dict[Path, str]:
    cargo_manifest_path = PROJECT_ROOT / "Cargo.toml"
    cargo_manifest = cargo_manifest_path.read_text(encoding="utf-8")
    match = re.search(
        r'(?ms)^\[workspace\.package\]\s*.*?^version\s*=\s*"(?P<version>[^"]+)"',
        cargo_manifest,
    )
    if match is None:
        raise RuntimeError("Could not read the workspace version")
    current = match.group("version")
    if parse_version(target) <= parse_version(current):
        raise ValueError(f"Target version {target} must be newer than {current}")

    escaped_current = re.escape(current)
    updates: dict[Path, str] = {
        cargo_manifest_path: replace_once(
            cargo_manifest,
            re.compile(
                rf'(?ms)(^\[workspace\.package\]\s*.*?^version\s*=\s*"){escaped_current}("\s*$)'
            ),
            rf"\g<1>{target}\g<2>",
            "the Cargo workspace version",
        )
    }

    cargo_package_pattern = re.compile(
        rf'(?m)(^name = "token-holdem-[^"]+"\r?\nversion = "){escaped_current}("$)'
    )
    for relative_path in ("Cargo.lock", "fuzz/Cargo.lock"):
        path = PROJECT_ROOT / relative_path
        text = path.read_text(encoding="utf-8")
        updated, count = cargo_package_pattern.subn(rf"\g<1>{target}\g<2>", text)
        if count != 6:
            raise RuntimeError(f"Expected six workspace packages in {path}, found {count}")
        updates[path] = updated

    for relative_path in (
        "plugins/token-holdem/.codex-plugin/plugin.json",
        "plugins/token-holdem/mcp/package.json",
        "ui/package.json",
    ):
        path = PROJECT_ROOT / relative_path
        updates[path] = update_json_manifest(path, current, target)

    for relative_path in (
        "plugins/token-holdem/mcp/package-lock.json",
        "ui/package-lock.json",
    ):
        path = PROJECT_ROOT / relative_path
        updates[path] = update_package_lock(path, current, target)

    return updates


def main() -> None:
    parser = argparse.ArgumentParser(description="Update every Token Poker release version")
    parser.add_argument("version", help="New stable semantic version")
    arguments = parser.parse_args()
    updates = prepare_updates(arguments.version)
    for path, content in updates.items():
        path.write_text(content, encoding="utf-8", newline="\n")
    print(f"Updated {len(updates)} files to Token Poker {arguments.version}.")


if __name__ == "__main__":
    main()
