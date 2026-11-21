#!/usr/bin/env python3
"""Unpack the IDM 6.43b18 proprietary zlib container (functional re-implementation
from the documented format in docs/RESEARCH-IDM-STUDY.md §1; clean-room tooling).

Format (verified on-disk 2026-11-21, reconciled against the study's functional
description):
  - marker b'!--END FILE--!' in the last 0x5000 bytes of the file
  - u32 table offset in the 4 bytes immediately before the marker
  - entries chain from that offset:
      [u32 usize (hint; entry 0's is anomalous)][u32 clen (authoritative)]
      [3 x FILETIME (8 bytes each)][clen bytes zlib stream]
  - entry 0 is a plain-text manifest naming the payload files
    (`<P#="name">` patterns); entries 1..N map to P1..PN in order.

Validation: each zlib stream must consume exactly clen bytes and end cleanly
(dec.eof); usize is a logged cross-check — entry 0's field does not match its
8697-byte manifest output (tolerated, recorded in chain-meta.txt).

Usage: unpack_idm.py <installer.exe> <outdir>
Output goes ONLY to a throwaway directory outside the repo.
"""
import re
import struct
import sys
import zlib
from pathlib import Path

MARKER = b"!--END FILE--!"
TAIL = 0x5000


def find_marker(data: bytes) -> int:
    tail = data[-TAIL:]
    pos = tail.find(MARKER)
    if pos < 0:
        raise SystemExit("FAIL-CLOSED: no end-marker found in the last 0x5000 bytes")
    return len(data) - TAIL + pos


def parse_chain(data: bytes, toff: int, hdr: int):
    """Parse the entry chain with the given header size. A valid entry's zlib
    stream consumes exactly clen bytes and ends cleanly (dec.eof). usize is a
    hint; mismatches are logged by the caller via chain-meta.txt."""
    out = []
    p = toff
    n = len(data)
    while p + hdr <= n:
        usize, clen = struct.unpack_from("<II", data, p)
        if clen == 0:
            break
        raw = data[p + hdr : p + hdr + clen]
        if len(raw) != clen:
            break  # ran past the data / hit the trailer region
        try:
            dec = zlib.decompressobj()
            plain = dec.decompress(raw)
            if not dec.eof:  # stream must end cleanly inside clen bytes
                break
        except zlib.error:
            break
        out.append((usize, clen, plain))
        p += hdr + clen
    return out, p


def main() -> None:
    src = Path(sys.argv[1])
    out = Path(sys.argv[2])
    data = src.read_bytes()
    mpos = find_marker(data)
    toff = struct.unpack_from("<I", data, mpos - 4)[0]
    print(f"marker@{mpos:#x} table_offset={toff:#x} file_size={len(data)}")

    best = None
    for hdr in (32, 20, 12):  # 3x8 FILETIME first; alternates as fallbacks
        entries, endp = parse_chain(data, toff, hdr)
        if not entries:
            continue
        if best is None or len(entries) > len(best[0]):
            best = (entries, endp, hdr)
    if not best:
        raise SystemExit("FAIL-CLOSED: no entry chain parsed under any header layout")
    entries, endp, hdr = best
    print(f"parsed {len(entries)} entries (header={hdr}B) chain ends @{endp:#x} "
          f"(file end - {len(data) - endp:#x})")

    out.mkdir(parents=True, exist_ok=True)
    for i, (usize, clen, plain) in enumerate(entries):
        (out / f"entry_{i:03d}.bin").write_bytes(plain)
    (out / "chain-meta.txt").write_text(
        "\n".join(
            f"{i}\tusize_hint={u}\tclen={c}\tout={len(pl)}"
            + ("\tUSIZE-MISMATCH" if u != len(pl) else "")
            for i, (u, c, pl) in enumerate(entries)
        )
        + "\n"
    )

    # entry 0 = text manifest: map P-numbers to destination names
    names = {}
    if entries:
        txt = entries[0][2].decode("cp1252", "replace")
        (out / "manifest.txt").write_text(txt)
        for m in re.finditer(r'<P(\d+)="([^"]+)"', txt):
            names[int(m.group(1))] = m.group(2)
    print(f"manifest names: {len(names)}")

    payload = out / "payload"
    payload.mkdir(exist_ok=True)
    mapped = 0
    for i in range(1, len(entries)):
        (payload / f"entry_{i:03d}.bin").write_bytes(entries[i][2])
        if i in names:
            (payload / names[i].replace("\\", "__")).write_bytes(entries[i][2])
            mapped += 1
    print(f"payloads written: {len(entries) - 1} (manifest-mapped: {mapped})")


if __name__ == "__main__":
    main()
