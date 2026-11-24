#!/usr/bin/env python3
"""S11 T2 throwaway extraction: pull every zlib stream from the IDM installer,
identify PEs via VS_VERSIONINFO, save to /tmp/vdm-t2/payload/. Clean-room:
extracts payloads for behavioral study only, artifacts wiped after use."""
import zlib, struct, os, re, sys

DATA = open('/tmp/vdm-t2/idman643build18.exe', 'rb').read()
OUT = '/tmp/vdm-t2/payload'
os.makedirs(OUT, exist_ok=True)

def u16le(b):
    return b.decode('utf-16-le', 'replace')

def version_info(data):
    """Extract FileDescription/ProductName/OriginalFilename/InternalName from a PE's .rsrc."""
    out = {}
    try:
        pe_off = struct.unpack('<I', data[0x3c:0x40])[0]
        if data[pe_off:pe_off+4] != b'PE\x00\x00':
            return out
        nsec = struct.unpack('<H', data[pe_off+6:pe_off+8])[0]
        opt_size = struct.unpack('<H', data[pe_off+20:pe_off+22])[0]
        sec0 = pe_off + 24 + opt_size
        rsrc = None
        for i in range(nsec):
            s = sec0 + 40*i
            name = data[s:s+8].rstrip(b'\x00')
            vsize, va = struct.unpack('<II', data[s+8:s+16])
            if name == b'.rsrc':
                rsrc = (va, vsize)
        if not rsrc:
            return out
        va, vsize = rsrc
        # brute-scan versioninfo marker inside resource section
        marker = b'V\x00S\x00_\x00V\x00E\x00R\x00S\x00I\x00O\x00N\x00'
        sect = data[va:va+min(vsize, 0x100000)]
        idx = sect.find(marker)
        if idx < 0:
            return out
        # walk key=value utf16 pairs after marker
        blob = sect[idx:idx+4000]
        for key in (b'F\x00i\x00l\x00e\x00D\x00e\x00s\x00c\x00r\x00i\x00p\x00t\x00i\x00o\x00n',
                    b'P\x00r\x00o\x00d\x00u\x00c\x00t\x00N\x00a\x00m\x00e',
                    b'O\x00r\x00i\x00g\x00i\x00n\x00a\x00l\x00F\x00i\x00l\x00e\x00n\x00a\x00m\x00e',
                    b'I\x00n\x00t\x00e\x00r\x00n\x00a\x00l\x00N\x00a\x00m\x00e',
                    b'F\x00i\x00l\x00e\x00V\x00e\x00r\x00s\x00i\x00o\x00n',
                    b'C\x00o\x00m\x00p\x00a\x00n\x00y\x00N\x00a\x00m\x00e'):
            k = blob.find(key)
            if k < 0:
                continue
            # skip to value (null, padding, then utf16 value)
            v = k + len(key)
            while v < len(blob) and blob[v:v+2] in (b'\x00\x00',):
                v += 2
            # value string: read utf16 until null
            end = blob.find(b'\x00\x00', v)
            val = u16le(blob[v:end if end > v else v+120])
            if val and not val.startswith(('V','S')) and len(val) > 1:
                out[u16le(key).rstrip('\x00')] = val
    except Exception:
        pass
    return out

streams = []
for m in re.finditer(rb'\x78[\x01\x5e\x9c\xda\xbb\x2f\x70\x11\xa4\x2c\xfe]', DATA):
    off = m.start()
    try:
        d = zlib.decompressobj()
        out = d.decompress(DATA[off:off+6_000_000], 12_000_000)
    except Exception:
        continue
    if len(out) > 4096 and out[:1] in (b'M',) or out[:4] in (b'PK\x03\x04', b'Cr24', b'ITSF') or out[:2] == b'BM':
        streams.append((off, out))

# dedup nested prefix streams (a stream found at a superset offset) — keep exact unique outputs
seen = set()
report = []
for off, out in sorted(streams, key=lambda t: -len(t[1])):
    h = (len(out), out[:64])
    if h in seen:
        continue
    seen.add(h)
    if out[:2] == b'MZ':
        vi = version_info(out)
        pe_off = struct.unpack('<I', out[0x3c:0x40])[0]
        mach = struct.unpack('<H', out[pe_off+4:pe_off+6])[0]
        name = vi.get('OriginalFilename') or vi.get('InternalName') or f'pe_{len(out)}.exe'
        name = name.strip('\x00') or f'pe_{len(out)}.exe'
        fn = f'{OUT}/{name}'
        i = 1
        while os.path.exists(fn):
            fn = f'{OUT}/{i:02d}_{name}'
            i += 1
        open(fn, 'wb').write(out)
        report.append((name, len(out), hex(mach), vi.get('FileDescription', ''), vi.get('ProductName', '')))
    else:
        tag = {b'PK\x03\x04': 'zip', b'Cr24': 'crx', b'ITSF': 'chm', b'BM': 'bmp'}.get(out[:4], out[:4].hex())
        fn = f'{OUT}/blob_{off:x}.{tag}'
        open(fn, 'wb').write(out)
        report.append((f'blob_{off:x}.{tag}', len(out), '', '', ''))

print(f'{"artifact":44s} {"size":>10s} machine  FileDescription / ProductName')
for name, sz, mach, fd, pn in sorted(report, key=lambda r: -r[1]):
    print(f'{name:44s} {sz:10d} {mach:6s}  {fd[:46]} / {pn[:30]}')
