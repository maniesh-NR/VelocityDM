#!/usr/bin/env python3
"""Generate VelocityDM extension icons (Material Design 3 style) as PNGs."""
import struct
import zlib
import sys
import os

def png_chunk(tag, data):
    return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', zlib.crc32(tag + data) & 0xffffffff)

def write_png(path, size):
    # canvas with anti-aliased drawing via supersampling
    ss = 4
    w = h = size * ss
    bg = (0x6750A4 >> 16 & 255, 0x6750A4 >> 8 & 255, 0x6750A4 & 255)  # MD3 primary
    fg = (0xD0BCFF >> 16 & 255, 0xD0BCFF >> 8 & 255, 0xD0BCFF & 255)  # light primary
    px = [[bg for _ in range(w)] for _ in range(h)]

    radius = int(0.22 * size) * ss
    def inside_round(x, y):
        # rounded-rect mask
        rr = radius
        if rr == 0:
            return True
        cx = min(max(x, rr), w - 1 - rr)
        cy = min(max(y, rr), h - 1 - rr)
        dx, dy = x - cx, y - cy
        return dx * dx + dy * dy <= rr * rr

    # clear outside the rounded rect to transparent
    for y in range(h):
        for x in range(w):
            if not inside_round(x, y):
                px[y][x] = None

    # draw a double-chevron "fast download" mark
    def fill_tri(x0, y0, x1, y1, x2, y2, color):
        minx, maxx = int(min(x0, x1, x2)), int(max(x0, x1, x2))
        miny, maxy = int(min(y0, y1, y2)), int(max(y0, y1, y2))
        for y in range(max(0, miny), min(h, maxy + 1)):
            for x in range(max(0, minx), min(w, maxx + 1)):
                d = ((x1 - x0) * (y - y0) - (y1 - y0) * (x - x0))
                d2 = ((x2 - x1) * (y - y1) - (y2 - y1) * (x - x1))
                d3 = ((x0 - x2) * (y - y2) - (y0 - y2) * (x - x2))
                if (d >= 0 and d2 >= 0 and d3 >= 0) or (d <= 0 and d2 <= 0 and d3 <= 0):
                    if 0 <= y < h and 0 <= x < w:
                        px[y][x] = color

    s = size * ss
    thick = int(s * 0.09)
    # two chevrons pointing down, upper smaller
    cy1 = 0.30 * s
    cy2 = 0.52 * s
    half = 0.26 * s
    for (cy, hh) in ((cy1, half * 0.8), (cy2, half)):
        fill_tri(s / 2 - hh, cy - thick, s / 2 + hh, cy - thick, s / 2, cy + thick * 0.4, fg)
    # base bar
    bx0, bx1 = s / 2 - half, s / 2 + half
    by = 0.68 * s
    for y in range(int(by - thick / 2), int(by + thick / 2)):
        for x in range(int(bx0), int(bx1)):
            if 0 <= y < h and 0 <= x < w and px[y][x] is not None:
                px[y][x] = fg

    # downsample with alpha
    rows = []
    for y in range(size):
        row = bytearray([0])  # filter type 0
        for x in range(size):
            r = g = b = a = n = 0
            for dy in range(ss):
                for dx in range(ss):
                    c = px[y * ss + dy][x * ss + dx]
                    if c is not None:
                        r += c[0]; g += c[1]; b += c[2]; n += 1
            if n == 0:
                row += bytes((0, 0, 0, 0))
            else:
                row += bytes((r // n, g // n, b // n, 255))
        rows.append(bytes(row))

    ihdr = struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0)
    data = b''.join(rows)
    with open(path, 'wb') as f:
        f.write(b'\x89PNG\r\n\x1a\n')
        f.write(png_chunk(b'IHDR', ihdr))
        f.write(png_chunk(b'IDAT', zlib.compress(data, 9)))
        f.write(png_chunk(b'IEND', b''))

if __name__ == '__main__':
    out_dir = sys.argv[1] if len(sys.argv) > 1 else 'icons'
    os.makedirs(out_dir, exist_ok=True)
    for s in (16, 48, 128):
        write_png(os.path.join(out_dir, f'icon{s}.png'), s)
        print(f'wrote {out_dir}/icon{s}.png')
