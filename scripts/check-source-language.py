from __future__ import annotations

import re
import sys
from pathlib import Path


PROJECT_ROOT = Path(__file__).resolve().parent.parent
HAN_PATTERN = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]")
LINE_COMMENT_PATTERN = re.compile(r"^\s*(?://[/!]?|#(?!\[))(?P<body>.*)$")
BLOCK_COMMENT_PATTERN = re.compile(r"/\*.*?\*/|<!--.*?-->", re.DOTALL)
SOURCE_SUFFIXES = {
    ".css",
    ".html",
    ".js",
    ".mjs",
    ".ps1",
    ".py",
    ".rs",
    ".toml",
    ".ts",
    ".tsx",
    ".yaml",
    ".yml",
}
IGNORED_DIRECTORIES = {
    ".git",
    ".playwright-cli",
    ".venv",
    "dist",
    "node_modules",
    "output",
    "specs",
    "target",
}
IGNORED_FILES = {
    Path("design-qa.md"),
    Path("docs/CODEX-DESIGN-AUDIT.md"),
    Path("docs/UI-FIDELITY.md"),
}
REQUIRED_ENGLISH_DOCUMENTS = {
    Path("README.md"),
    Path("CONTRIBUTING.md"),
    Path("CODE_OF_CONDUCT.md"),
    Path("SECURITY.md"),
    Path("docs/COMMUNITY-NODE.md"),
    Path("docs/HAND-PROTOCOL.md"),
    Path("docs/MATCHMAKING.md"),
    Path("docs/PLUGIN-DISTRIBUTION.md"),
    Path("fuzz/README.md"),
}


def is_ignored(path: Path) -> bool:
    relative = path.relative_to(PROJECT_ROOT)
    return relative in IGNORED_FILES or any(
        part in IGNORED_DIRECTORIES for part in relative.parts
    )


def scan_document(path: Path) -> list[tuple[int, str]]:
    violations: list[tuple[int, str]] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if HAN_PATTERN.search(line):
            violations.append((line_number, line.strip()))
    return violations


def scan_source_comments(path: Path) -> list[tuple[int, str]]:
    text = path.read_text(encoding="utf-8")
    violations: list[tuple[int, str]] = []
    lines = text.splitlines()
    for line_number, line in enumerate(lines, 1):
        match = LINE_COMMENT_PATTERN.match(line)
        if match is not None and HAN_PATTERN.search(match.group("body")):
            violations.append((line_number, line.strip()))

    for match in BLOCK_COMMENT_PATTERN.finditer(text):
        comment = match.group(0)
        if HAN_PATTERN.search(comment):
            line_number = text.count("\n", 0, match.start()) + 1
            violations.append((line_number, comment.splitlines()[0].strip()))
    return violations


def main() -> int:
    violations: list[tuple[Path, int, str]] = []

    for relative_path in sorted(REQUIRED_ENGLISH_DOCUMENTS):
        path = PROJECT_ROOT / relative_path
        if not path.is_file():
            violations.append((relative_path, 0, "required English document is missing"))
    for path in sorted(PROJECT_ROOT.rglob("*.md")):
        if not path.is_file() or is_ignored(path):
            continue
        relative_path = path.relative_to(PROJECT_ROOT)
        violations.extend(
            (relative_path, line_number, line)
            for line_number, line in scan_document(path)
        )

    for path in sorted(PROJECT_ROOT.rglob("*")):
        if not path.is_file() or is_ignored(path) or path.suffix not in SOURCE_SUFFIXES:
            continue
        relative_path = path.relative_to(PROJECT_ROOT)
        violations.extend(
            (relative_path, line_number, line)
            for line_number, line in scan_source_comments(path)
        )

    if violations:
        print("Chinese text is not allowed in public English documentation or source comments:")
        for path, line_number, line in violations:
            location = f"{path}:{line_number}" if line_number else str(path)
            print(f"- {location}: {line}")
        return 1

    print("Public documentation and source comments are English-only.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
