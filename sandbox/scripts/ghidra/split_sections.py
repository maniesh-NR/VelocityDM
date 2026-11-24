#!/usr/bin/env python3
"""Split a S11Decompile.java decomp-targets.c evidence file into per-tag sections.

Usage: split_sections.py <decomp-targets.c> <outdir>
Sections start at '/* ===== S11_<tag> @ <addr> body=N ===== */' markers.
Output files are named <seq>_<tag>.c so multiple tags sharing one function
(r2_bytes_fmt+r2_bytes_fmt2) keep their first-tag filename.
"""
import re
import sys
from pathlib import Path

src = Path(sys.argv[1]).read_text(encoding="utf-8", errors="replace")
out = Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)

marker = re.compile(r"/\* ===== S11_(\S+?) @ ([0-9a-f]+) body=(\d+) ===== \*/")
hits = list(marker.finditer(src))
for i, m in enumerate(hits):
    start = m.start()
    end = hits[i + 1].start() if i + 1 < len(hits) else len(src)
    body = src[start:end].rstrip() + "\n"
    tag = m.group(1)
    (out / f"{i:03d}_{tag}_{m.group(2)}.c").write_text(body, encoding="utf-8")
print(f"wrote {len(hits)} sections to {out}")
