#!/usr/bin/env python3
"""Annotate no-analysis Ghidra decompiled sections with string literals.

Replaces UNK_00xxxxxx / DAT_00xxxxxx / s_00xxxxxx style references whose
address appears in a S11DumpStrings.java strings.tsv dump with a short
inline literal marker, e.g.  UNK_00712060  ->  UNK_00712060["..."].
Unknown addresses are left untouched (fail-open annotation, evidence only).

Usage: s11_annotate.py <strings.tsv> <sectionsDir> <outDir> [maxLiteral=70]
"""
import sys
from pathlib import Path

tsv, sdir, odir = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
maxlit = int(sys.argv[4]) if len(sys.argv) > 4 else 70

strmap = {}
for line in Path(tsv).read_text(encoding="utf-8", errors="replace").splitlines():
    addr, _, text = line.partition("\t")
    if addr and text:
        strmap[addr.lower()] = text

import re
pat = re.compile(r"\b(?:UNK|DAT|s)_?0*([0-9a-fA-F]{8})\b")
odir.mkdir(parents=True, exist_ok=True)
annotated = subbed = 0
for f in sorted(sdir.glob("*.c")):
    text = f.read_text(encoding="utf-8", errors="replace")

    def rep(m):
        global subbed
        key = m.group(1).lstrip("0").lower() or "0"
        lit = strmap.get(key)
        if lit is None:
            return m.group(0)
        subbed += 1
        shown = lit if len(lit) <= maxlit else lit[:maxlit] + "..."
        shown = shown.replace('"', "'")
        return f'{m.group(0)}["{shown}"]'

    new = pat.sub(rep, text)
    if new != text:
        annotated += 1
    (odir / f.name).write_text(new, encoding="utf-8")
print(f"annotated {annotated}/{len(list(sdir.glob('*.c')))} sections, "
      f"{subbed} literal refs resolved (map={len(strmap)})")
