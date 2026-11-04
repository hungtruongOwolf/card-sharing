#!/usr/bin/env python3
"""Sync context/wiki/progress/backlog.json to GitHub labels, milestones and issues (idempotent).

- Labels: created if missing (colour by prefix).
- Milestones: "M0 — <name>" from tickets.MILESTONES, created if missing.
- Issues: title "[T-n] <title>"; an issue whose title starts with "[T-n]" is never created twice.
- Dependencies: a second pass rewrites "Depends on: T-x" into "Depends on: T-x (#123)" links.

Usage: python tools/planning/sync_github.py [--dry-run]
Requires an authenticated `gh` CLI with access to the current repo.
"""
from __future__ import annotations

import json
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
from tickets import MILESTONES  # noqa: E402

DRY = "--dry-run" in sys.argv
PAUSE_S = 1.2  # stay under GitHub's secondary rate limit for content creation
COLORS = {"area": "1d76db", "milestone": "5319e7", "type": "0e8a16", "priority": "d93f0b", "sprint": "fbca04",
          "timebox": "c5def5", "status": "bfdadc"}
DEFAULT_COLOR = "ededed"


def gh(*args: str, input_text: str | None = None) -> str:
    if DRY and args[0] != "api" and args[:2] not in (("issue", "list"), ("label", "list")):
        print("DRY:", "gh", *args[:4])
        return ""
    result = subprocess.run(["gh", *args], input=input_text, capture_output=True, text=True, encoding="utf-8")
    if result.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args[:4])} failed: {result.stderr.strip()}")
    return result.stdout


def ensure_labels(backlog: list[dict]) -> None:
    existing = {l["name"] for l in json.loads(gh("label", "list", "--limit", "500", "--json", "name"))}
    for name in sorted({l for t in backlog for l in t["labels"]} - existing):
        color = COLORS.get(name.split(":")[0].split("-")[0], DEFAULT_COLOR)
        gh("label", "create", name, "--color", color)
        print("label +", name)
        time.sleep(PAUSE_S)


def ensure_milestones() -> dict[str, int]:
    repo = gh("repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner").strip() if not DRY else \
        subprocess.run(["gh", "repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"],
                       capture_output=True, text=True).stdout.strip()
    existing = {m["title"]: m["number"] for m in json.loads(gh("api", f"repos/{repo}/milestones?state=all&per_page=100"))}
    numbers: dict[str, int] = {}
    for key, name in MILESTONES.items():
        title = f"{key} — {name}"
        if title not in existing and not DRY:
            created = json.loads(gh("api", f"repos/{repo}/milestones", "-f", f"title={title}",
                                    "-f", f"description=See context/wiki/progress/mvp-plan.md ({key})"))
            existing[title] = created["number"]
            print("milestone +", title)
            time.sleep(PAUSE_S)
        numbers[key] = existing.get(title, 0)
    return numbers


def existing_issues() -> dict[str, int]:
    issues = json.loads(gh("issue", "list", "--state", "all", "--limit", "1000", "--json", "number,title"))
    found = {}
    for issue in issues:
        match = re.match(r"\[(T-\d+)\]", issue["title"])
        if match:
            found[match.group(1)] = issue["number"]
    return found


def main() -> int:
    backlog = json.loads((ROOT / "context/wiki/progress/backlog.json").read_text(encoding="utf-8"))
    ensure_labels(backlog)
    ensure_milestones()
    numbers = existing_issues()

    for ticket in backlog:
        if ticket["id"] in numbers:
            continue
        title = f"[{ticket['id']}] {ticket['title']}"
        milestone = f"{ticket['milestone']} — {MILESTONES[ticket['milestone']]}"
        out = gh("issue", "create", "--title", title, "--body-file", "-", "--milestone", milestone,
                 *[arg for label in ticket["labels"] for arg in ("--label", label)], input_text=ticket["body_markdown"])
        if out:
            numbers[ticket["id"]] = int(out.strip().rsplit("/", 1)[-1])
            print("issue +", title, "->", numbers[ticket["id"]])
        time.sleep(PAUSE_S)

    # Pass 2: link dependencies to issue numbers.
    for ticket in backlog:
        if not ticket["depends_on"] or ticket["id"] not in numbers:
            continue
        links = ", ".join(f"{dep} (#{numbers[dep]})" if dep in numbers else dep for dep in ticket["depends_on"])
        body = re.sub(r"^- Depends on: .*$", f"- Depends on: {links}", ticket["body_markdown"], count=1, flags=re.M)
        if body != ticket["body_markdown"]:
            gh("issue", "edit", str(numbers[ticket["id"]]), "--body-file", "-", input_text=body)
            time.sleep(PAUSE_S)
    print(f"done: {len(numbers)} issues tracked")
    return 0


if __name__ == "__main__":
    sys.exit(main())
