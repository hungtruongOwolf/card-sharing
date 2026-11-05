"""Rebuild the static brand font faces in apps/mobile/assets/fonts from the official google/fonts files.

Expo SDK 57 supports only static font files (variable fonts arrive in SDK 58), but google/fonts ships
Bricolage Grotesque, DM Sans and Caveat as variable fonts only. This pins the weights design-system.md
section 3.2 uses (the same instancing Google Fonts applies when it serves static files). Anton and DM Mono are
static upstream and are copied as-is. The SIL OFL 1.1 has no Reserved Font Name for these families, so the
derived files keep their names; each family folder carries its OFL.txt.

Usage (fontTools in a throwaway venv, never a repo dependency):
    python -m venv .venv && .venv/bin/pip install fonttools
    curl -LO "https://raw.githubusercontent.com/google/fonts/main/ofl/bricolagegrotesque/BricolageGrotesque[opsz,wdth,wght].ttf"
    ...  (DMSans[opsz,wght].ttf, Caveat[wght].ttf), renamed to <Family>-VF.ttf
    .venv/bin/python instance_fonts.py <dir with *-VF.ttf> <output dir>

Optical size is pinned to the size range each face serves: Bricolage SemiBold (titles, 20-24 pt) at
opsz 24, Bricolage Bold (display, 32-40 pt) at opsz 36, DM Sans (body and labels, 12-17 pt) at opsz 14.
"""
import os
import sys

from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

src, out = sys.argv[1], sys.argv[2]
JOBS = [
    ("BricolageGrotesque-VF.ttf", {"opsz": 24, "wght": 600, "wdth": 100}, "Bricolage Grotesque", "SemiBold", 600),
    ("BricolageGrotesque-VF.ttf", {"opsz": 36, "wght": 700, "wdth": 100}, "Bricolage Grotesque", "Bold", 700),
    ("DMSans-VF.ttf", {"opsz": 14, "wght": 400}, "DM Sans", "Regular", 400),
    ("DMSans-VF.ttf", {"opsz": 14, "wght": 600}, "DM Sans", "SemiBold", 600),
    ("Caveat-VF.ttf", {"wght": 500}, "Caveat", "Medium", 500),
]
for file, loc, family, style, weight in JOBS:
    font = instantiateVariableFont(TTFont(os.path.join(src, file)), loc)
    ps = family.replace(" ", "") + "-" + style
    name = font["name"]
    # Drop old names and write a consistent static set: typographic family/subfamily + legacy RIBBI.
    for nid in (1, 2, 3, 4, 6, 16, 17, 21, 22, 25):
        name.removeNames(nameID=nid)
    legacy_style = "Bold" if style == "Bold" else "Regular"
    legacy_family = family if style in ("Regular", "Bold") else f"{family} {style}"
    name.setName(legacy_family, 1, 3, 1, 0x409)
    name.setName(legacy_style, 2, 3, 1, 0x409)
    name.setName(f"{ps};instanced from google/fonts variable font", 3, 3, 1, 0x409)
    name.setName(f"{family} {style}", 4, 3, 1, 0x409)
    name.setName(ps, 6, 3, 1, 0x409)
    if legacy_family != family:
        name.setName(family, 16, 3, 1, 0x409)
        name.setName(style, 17, 3, 1, 0x409)
    font["OS/2"].usWeightClass = weight
    sel = font["OS/2"].fsSelection & ~0b1100001  # clear italic, bold, regular
    sel |= 0b100000 if style == "Bold" else 0b1000000
    font["OS/2"].fsSelection = sel
    font["head"].macStyle = 1 if style == "Bold" else 0
    font.save(os.path.join(out, ps + ".ttf"))
    print(ps, loc)
