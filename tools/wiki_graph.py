#!/usr/bin/env python3
"""Context-wiki relationship checker (same [[wikilink]] model as Obsidian).

Usage:
  python tools/wiki_graph.py lint              # broken links, orphans, pages missing from index, missing front-matter
  python tools/wiki_graph.py impact <page>     # everything that links to <page> (direct + transitive) -> must be reviewed on change
  python tools/wiki_graph.py links <page>      # outgoing links of <page>

Exit code 1 if lint finds problems (usable in CI / pre-commit).
"""
from __future__ import annotations

import re
import sys
from collections import defaultdict, deque
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "context"
WIKI = ROOT / "wiki"
INDEX = ROOT / "index.md"
LINK_RE = re.compile(r"\[\[([^\]|#]+)(?:[#|][^\]]*)?\]\]")
REQUIRED_FM = ("title", "type", "status", "updated")


def load_pages() -> dict[str, Path]:
    pages: dict[str, Path] = {}
    for path in WIKI.rglob("*.md"):
        name = path.stem
        if name in pages:
            print(f"DUPLICATE page name '{name}': {pages[name]} and {path}")
        pages[name] = path
    return pages


CODE_RE = re.compile(r"```.*?```|`[^`\n]*`", re.DOTALL)


def outgoing(path: Path) -> set[str]:
    text = CODE_RE.sub("", path.read_text(encoding="utf-8"))  # links inside code are examples, not links
    return {m.strip() for m in LINK_RE.findall(text)}


def front_matter(path: Path) -> dict[str, str]:
    text = path.read_text(encoding="utf-8")
    if not text.startswith("---"):
        return {}
    block = text.split("---", 2)[1]
    fm = {}
    for line in block.splitlines():
        if ":" in line:
            key, value = line.split(":", 1)
            fm[key.strip()] = value.strip()
    return fm


def build_graph(pages: dict[str, Path]) -> tuple[dict[str, set[str]], dict[str, set[str]]]:
    out = {name: outgoing(path) - {name} for name, path in pages.items()}
    inbound: dict[str, set[str]] = defaultdict(set)
    for src, targets in out.items():
        for target in targets:
            inbound[target].add(src)
    return out, inbound


def lint() -> int:
    pages = load_pages()
    out, inbound = build_graph(pages)
    index_links = outgoing(INDEX)
    problems = 0

    for src, targets in sorted(out.items()):
        for target in sorted(targets - pages.keys()):
            print(f"BROKEN   {src} -> [[{target}]]")
            problems += 1
    for target in sorted(index_links - pages.keys()):
        print(f"BROKEN   index -> [[{target}]]")
        problems += 1
    for name in sorted(pages):
        if name not in index_links:
            print(f"UNINDEXED {name} ({pages[name].relative_to(ROOT)})")
            problems += 1
        if not inbound.get(name) and name != "adr-template":
            print(f"ORPHAN   {name} (no wiki page links to it)")
            problems += 1
        missing = [k for k in REQUIRED_FM if k not in front_matter(pages[name])]
        if missing:
            print(f"FRONTMATTER {name} missing {missing}")
            problems += 1

    print(f"\n{len(pages)} pages, {sum(len(v) for v in out.values())} links, {problems} problem(s)")
    return 1 if problems else 0


def impact(page: str) -> int:
    pages = load_pages()
    if page not in pages:
        print(f"unknown page '{page}'")
        return 1
    _, inbound = build_graph(pages)
    seen, queue = {page: 0}, deque([page])
    while queue:
        current = queue.popleft()
        for src in inbound.get(current, ()):
            if src not in seen:
                seen[src] = seen[current] + 1
                queue.append(src)
    print(f"Pages to review when '{page}' changes (distance = hops):")
    for name, dist in sorted(seen.items(), key=lambda kv: (kv[1], kv[0])):
        if name != page:
            print(f"  {dist}  {name:40} {pages[name].relative_to(ROOT)}")
    return 0


def links(page: str) -> int:
    pages = load_pages()
    if page not in pages:
        print(f"unknown page '{page}'")
        return 1
    for target in sorted(outgoing(pages[page])):
        print(f"  -> {target}{'' if target in pages else '   (BROKEN)'}")
    return 0


if __name__ == "__main__":
    args = sys.argv[1:]
    if not args or args[0] == "lint":
        sys.exit(lint())
    if args[0] in ("impact", "links") and len(args) == 2:
        sys.exit(impact(args[1]) if args[0] == "impact" else links(args[1]))
    print(__doc__)
    sys.exit(2)
