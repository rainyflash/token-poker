from __future__ import annotations

from pathlib import Path
import tomllib


ROOT = Path(__file__).resolve().parents[1]
LOCKFILES = (ROOT / "Cargo.lock", ROOT / "fuzz" / "Cargo.lock")
MINIMUM_SAFE_VERSIONS = {
    "yamux": (0, 13, 10),
    "hickory-proto": (0, 26, 1),
}


def parse_release_version(value: str) -> tuple[int, int, int]:
    release = value.split("-", maxsplit=1)[0]
    parts = release.split(".")
    if len(parts) != 3 or any(not part.isdecimal() for part in parts):
        raise ValueError(f"Unsupported package version: {value}")
    return int(parts[0]), int(parts[1]), int(parts[2])


def validate_lockfile(path: Path) -> None:
    with path.open("rb") as lockfile:
        document = tomllib.load(lockfile)

    packages = document.get("package")
    if not isinstance(packages, list):
        raise RuntimeError(f"{path.relative_to(ROOT)} has no package table")

    versions_by_name: dict[str, list[str]] = {name: [] for name in MINIMUM_SAFE_VERSIONS}
    for package in packages:
        if not isinstance(package, dict):
            continue
        name = package.get("name")
        version = package.get("version")
        if name in versions_by_name and isinstance(version, str):
            versions_by_name[name].append(version)

    failures: list[str] = []
    for name, minimum in MINIMUM_SAFE_VERSIONS.items():
        versions = versions_by_name[name]
        if not versions:
            failures.append(f"required package {name} is absent")
            continue
        vulnerable = [version for version in versions if parse_release_version(version) < minimum]
        if vulnerable:
            required = ".".join(str(part) for part in minimum)
            failures.append(f"{name} {', '.join(vulnerable)} is below {required}")

    if failures:
        details = "; ".join(failures)
        raise RuntimeError(f"{path.relative_to(ROOT)} failed the Rust security baseline: {details}")


def main() -> None:
    for lockfile in LOCKFILES:
        validate_lockfile(lockfile)
    print("Rust lockfiles satisfy the pinned security baseline.")


if __name__ == "__main__":
    main()
