import json
import sys
from collections import OrderedDict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from tickets import MILESTONES, R, T  # noqa: E402

ROOT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[2]
OUT = ROOT / "context" / "wiki" / "progress"

MUST_US = ["US-1", "US-2", "US-3", "US-4", "US-5", "US-5a", "US-6", "US-7", "US-8", "US-10", "US-11", "US-13", "US-14",
           "US-15", "US-16", "US-17", "US-18", "US-19", "US-21", "US-23", "US-24", "US-26", "US-27", "US-28", "US-29",
           "US-30", "US-31"]
SHOULD_US = ["US-9", "US-12", "US-20", "US-25"]
COULD_US = ["US-22"]
ALL_SR = [f"SR-{i}" for i in range(1, 38)]

# ---- assign ids
ids = {}
for n, tk in enumerate(T, start=1):
    if tk["key"] in ids:
        raise SystemExit(f"duplicate key {tk['key']}")
    ids[tk["key"]] = f"T-{n}"
for tk in T:
    for d in tk["deps"]:
        if d not in ids:
            raise SystemExit(f"{tk['key']}: unknown dep {d}")
    for r in tk["refs"]:
        assert r in R, r
    assert tk["size"] in ("S", "M") or tk["size"].startswith("timebox:"), tk["key"]
    assert tk["ms"] in MILESTONES

# dependency order sanity: deps must have lower ids (DAG by construction)
for tk in T:
    for d in tk["deps"]:
        if int(ids[d][2:]) >= int(ids[tk["key"]][2:]):
            raise SystemExit(f"{tk['key']} depends on later ticket {d}")

# ---- coverage
us_map = OrderedDict((u, []) for u in MUST_US + SHOULD_US + COULD_US)
sr_map = OrderedDict((s, []) for s in ALL_SR)
for tk in T:
    for u in tk["stories"]:
        us_map.setdefault(u, []).append(ids[tk["key"]])
    for s in tk["srs"]:
        sr_map[s].append(ids[tk["key"]])
missing = [u for u in MUST_US + SHOULD_US if not us_map[u]] + [s for s in ALL_SR if not sr_map[s]]
if missing:
    raise SystemExit(f"uncovered: {missing}")

AREA = {"backend": "area:backend", "mobile": "area:mobile", "native": "area:native", "web": "area:web",
        "infra": "area:infra", "admin": "area:admin"}


def labels(tk):
    out = [AREA[tk["area"]], f"milestone:{tk['ms'].lower()}", f"type:{tk['type']}", f"priority:{tk['prio']}"]
    if tk["sprint"]:
        out.append(f"sprint-{tk['sprint']}")
    out += tk["extra"]
    seen = []
    for label in out:
        if label not in seen:
            seen.append(label)
    return seen


def dep_ids(tk):
    return [ids[d] for d in tk["deps"]]


def body(tk):
    tid = ids[tk["key"]]
    lines = [
        f"**{tid}** - Milestone **{tk['ms']} {MILESTONES[tk['ms']]}** - Type: {tk['type']} - Size: {tk['size']} - Priority: {tk['prio']}",
        "",
        f"- Stories: {', '.join(tk['stories']) or 'none (enabler)'}",
        f"- Security requirements: {', '.join(tk['srs']) or 'none'}",
        f"- Depends on: {', '.join(dep_ids(tk)) or 'none'}",
        "",
    ]
    if tk["type"] == "spike":
        lines += ["### Spike"] + [f"- {s}" for s in tk["scope"]] + ["", "### Expected output"]
        lines += [f"- [ ] {a}" for a in tk["acc"]]
        lines += ["- [ ] Findings summarized for the EM; durable facts ingested into the wiki (memory skill); follow-up tickets adjusted."]
    else:
        lines += ["### Scope"] + [f"- {s}" for s in tk["scope"]] + ["", "### Acceptance criteria"]
        lines += [f"- [ ] {a}" for a in tk["acc"]]
        lines += ["- [ ] Definition of Done per context/wiki/engineering/conventions.md (tests incl. authz-negative where an endpoint is touched, lint/type-check clean, OpenAPI + wiki updated, change impact covered, reviewed)."]
    lines += ["", "### References"] + [f"- {R[r]}" for r in tk["refs"]]
    lines += ["- context/wiki/progress/mvp-plan.md"]
    if tk["risk"]:
        lines += ["", "### Risk / notes", f"- {tk['risk']}"]
    return "\n".join(lines) + "\n"


backlog = []
for tk in T:
    backlog.append(OrderedDict([
        ("id", ids[tk["key"]]), ("title", tk["title"]), ("milestone", tk["ms"]), ("type", tk["type"]),
        ("stories", tk["stories"]), ("security_reqs", tk["srs"]), ("size", tk["size"]),
        ("depends_on", dep_ids(tk)), ("labels", labels(tk)), ("body_markdown", body(tk)),
    ]))
(OUT / "backlog.json").write_text(json.dumps(backlog, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")

# ---- generated markdown fragments
def esc(s):
    return s.replace("|", "\\|")


ms_index = []
for ms, name in MILESTONES.items():
    rows = [tk for tk in T if tk["ms"] == ms]
    ms_index.append(f"#### {ms} - {name} ({len(rows)} tickets)")
    ms_index.append("| Id | Title | Type | Size | Lane | Depends on |")
    ms_index.append("|---|---|---|---|---|---|")
    for tk in rows:
        ms_index.append(f"| {ids[tk['key']]} | {esc(tk['title'])} | {tk['type']} | {tk['size']} | {tk['area']} | {', '.join(dep_ids(tk)) or '-'} |")
    ms_index.append("")

prio = {u: "Must" for u in MUST_US} | {u: "Should" for u in SHOULD_US} | {u: "Could" for u in COULD_US}
trace_us = ["| Story | Priority | Tickets |", "|---|---|---|"]
for u, tl in us_map.items():
    trace_us.append(f"| {u} | {prio.get(u, '-')} | {', '.join(tl)} |")
trace_sr = ["| SR | Tickets |", "|---|---|"]
for s, tl in sr_map.items():
    trace_sr.append(f"| {s} | {', '.join(tl)} |")

counts = OrderedDict()
for tk in T:
    counts[tk["type"]] = counts.get(tk["type"], 0) + 1
summary = f"{len(T)} tickets ({', '.join(f'{v} {k}' for k, v in counts.items())})"

# sprint 1 blocks
s1 = [tk for tk in T if tk["sprint"] == 1]
blocks = []
for tk in s1:
    tid = ids[tk["key"]]
    story = ", ".join(tk["stories"]) or "enabler"
    sr = f" | SR: {', '.join(tk['srs'])}" if tk["srs"] else ""
    blocks.append(f"### {tid}: {tk['title']}" + ("  (SPIKE)" if tk["type"] == "spike" else ""))
    blocks.append(f"- Story: {story} | Size: {tk['size']}{sr} | Lane: {tk['area']}")
    if tk["type"] == "spike":
        blocks.append(f"- {tk['scope'][0]}")
        blocks.append(f"- {tk['scope'][1]}")
        blocks.append("- Expected output: " + " ".join(tk["acc"]))
    else:
        blocks.append("- Scope: " + " ".join(tk["scope"]))
        blocks.append("- Acceptance: " + " ".join(tk["acc"]))
    blocks.append(f"- Depends on: {', '.join(dep_ids(tk)) or 'none'}")
    blocks.append(f"- Risk/notes: {tk['risk'] or '-'}")
    blocks.append("")


def days(tk):
    if tk["size"] == "S":
        return 0.5
    if tk["size"] == "M":
        return 1.0
    return float(tk["size"].split(":")[1].rstrip("d"))


lane_days = OrderedDict()
for tk in s1:
    lane_days[tk["area"]] = lane_days.get(tk["area"], 0) + days(tk)
lane_table = ["| Lane | Tickets | Planned ideal days |", "|---|---|---|"]
for lane, d in lane_days.items():
    lane_table.append(f"| {lane} | {', '.join(ids[t['key']] for t in s1 if t['area'] == lane)} | {d:g} |")

frag = {
    "{{MS_INDEX}}": "\n".join(ms_index),
    "{{TRACE_US}}": "\n".join(trace_us),
    "{{TRACE_SR}}": "\n".join(trace_sr),
    "{{SUMMARY}}": summary,
    "{{S1_BLOCKS}}": "\n".join(blocks),
    "{{S1_LANES}}": "\n".join(lane_table),
    "{{S1_COUNT}}": str(len(s1)),
    "{{S1_SPIKES}}": str(sum(1 for t in s1 if t["type"] == "spike")),
}
for key in list(ids):
    frag["{{" + key + "}}"] = ids[key]
for ms in MILESTONES:
    rows = [ids[tk["key"]] for tk in T if tk["ms"] == ms]
    frag["{{RANGE_" + ms + "}}"] = f"{rows[0]} to {rows[-1]} ({len(rows)} tickets)"
frag["{{TOTAL}}"] = str(len(T))

for name in ("mvp-plan.md", "sprint-1.md"):
    text = (Path(__file__).parent / f"{name}.tpl").read_text(encoding="utf-8")
    for k, v in frag.items():
        text = text.replace(k, v)
    if "{{" in text:
        raise SystemExit(f"unresolved placeholder in {name}: {text[text.index('{{'):text.index('{{') + 40]}")
    (OUT / name).write_text(text, encoding="utf-8")

print(summary, "| sprint-1:", len(s1), "tickets |", dict(lane_days))
