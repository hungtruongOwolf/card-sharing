#!/usr/bin/env python3
"""Fail if Vietnamese text appears in repository files.

Project rule (CLAUDE.md): the EM chats in Vietnamese, but everything committed — code, comments,
docs, wiki, skills, commit-able config — is written in English.

Usage: python tools/english_only.py [paths...]   (default: whole repo)
Exit code 1 when Vietnamese is found (usable in CI / pre-commit).
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TEXT_SUFFIXES = {".md", ".py", ".rs", ".ts", ".tsx", ".js", ".mjs", ".cjs", ".json", ".yaml", ".yml",
                 ".toml", ".sql", ".sh", ".css", ".html", ".txt", ".env", ".example"}
SKIP_DIRS = {"node_modules", "target", ".git", ".next", ".expo", "dist", "build", ".turbo"}
# Vietnamese-specific letters (excludes characters shared with common Western European words).
VIET_CHARS = re.compile(r"[ạảãầấậẩẫăằắặẳẵẹẻẽềếệểễịỉĩọỏõồốộổỗơờớợởỡụủũưừứựửữỳỵỷỹđ]", re.IGNORECASE)
# Common Vietnamese words written without diacritics.
VIET_ASCII = re.compile(r"\b(chia task|lam gi|nhe|duoc|khong|cai nay|tieng viet)\b", re.IGNORECASE)


def iter_files(paths: list[Path]):
    for base in paths:
        if base.is_file():
            yield base
            continue
        for path in base.rglob("*"):
            if path.is_file() and not (set(path.relative_to(ROOT).parts) & SKIP_DIRS) \
                    and (path.suffix in TEXT_SUFFIXES or path.name.startswith(".env")):
                yield path


def main(argv: list[str]) -> int:
    paths = [Path(p).resolve() for p in argv] or [ROOT]
    hits = 0
    for path in iter_files(paths):
        if path.name == "settings.local.json" or path == Path(__file__).resolve():
            continue
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except (UnicodeDecodeError, OSError):
            continue
        for number, line in enumerate(lines, 1):
            if VIET_CHARS.search(line) or VIET_ASCII.search(line):
                hits += 1
                sys.stdout.buffer.write(f"{path.relative_to(ROOT)}:{number}: {line.strip()[:120]}\n".encode("utf-8"))
    print(f"{hits} line(s) with Vietnamese text")
    return 1 if hits else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
