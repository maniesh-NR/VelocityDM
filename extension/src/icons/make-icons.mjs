// SPDX-License-Identifier: GPL-3.0-or-later
// Icon generator for VelocityDM Companion.
//
// Produces icon-16/32/48/128.png. The PNG encoder below is self-contained:
// raw RGBA scanlines (filter type 0), zlib deflate via node:zlib, and a CRC32
// implemented from scratch. Drawing uses a 3x3 supersampled coverage sampler
// over two shapes: a flat #4285F4 rounded square and a simple white "V" built
// from two diagonal pixel bands. If anything fails during drawing, the
// generator falls back to solid-color squares so a build never breaks.

import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const BRAND = Object.freeze({ r: 66, g: 133, b: 244 }); // #4285F4
const WHITE = Object.freeze({ r: 255, g: 255, b: 255 });
const SIZES = [16, 32, 48, 128];

// --- CRC32 (RFC 1952 polynomial, reflected) --------------------------------

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(bytes) {
  let c = 0xffffffff;
  for (let i = 0; i < bytes.length; i += 1) c = CRC_TABLE[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

// --- minimal PNG encoder (color type 6, bit depth 8) ------------------------

function pngChunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length >>> 0, 0);
  const typeAndData = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(typeAndData), 0);
  return Buffer.concat([length, typeAndData, crc]);
}

function encodePng(width, height, rgba) {
  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width >>> 0, 0);
  header.writeUInt32BE(height >>> 0, 4);
  header[8] = 8; // bit depth
  header[9] = 6; // color type: RGBA
  header[10] = 0; // compression: deflate
  header[11] = 0; // filter method: adaptive
  header[12] = 0; // interlace: none
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y += 1) {
    raw[y * (stride + 1)] = 0; // filter type: none for every scanline
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  const idat = deflateSync(raw, { level: 9 });
  return Buffer.concat([signature, pngChunk("IHDR", header), pngChunk("IDAT", idat), pngChunk("IEND", Buffer.alloc(0))]);
}

// --- shape coverage ---------------------------------------------------------

function insideRoundedSquare(x, y, size) {
  if (x < 0 || y < 0 || x > size || y > size) return false;
  const radius = size * 0.22;
  const cx = Math.min(Math.max(x, radius), size - radius);
  const cy = Math.min(Math.max(y, radius), size - radius);
  const dx = x - cx;
  const dy = y - cy;
  return dx * dx + dy * dy <= radius * radius;
}

// The "V" glyph: two diagonal bands that converge at the bottom center.
function insideGlyph(x, y, size) {
  const top = size * 0.26;
  const bottom = size * 0.74;
  if (y < top || y > bottom) return false;
  const t = (y - top) / (bottom - top);
  const center = size * 0.5;
  const halfSpread = size * 0.19;
  const halfWidth = size * 0.085;
  const drop = halfSpread * (1 - t);
  const leftX = center - drop;
  const rightX = center + drop;
  return Math.abs(x - leftX) <= halfWidth || Math.abs(x - rightX) <= halfWidth;
}

function drawGlyphTile(size) {
  const rgba = Buffer.alloc(size * size * 4);
  const subsamples = 3;
  const total = subsamples * subsamples;
  for (let py = 0; py < size; py += 1) {
    for (let px = 0; px < size; px += 1) {
      let inside = 0;
      let glyph = 0;
      for (let sy = 0; sy < subsamples; sy += 1) {
        for (let sx = 0; sx < subsamples; sx += 1) {
          const x = px + (sx + 0.5) / subsamples;
          const y = py + (sy + 0.5) / subsamples;
          if (!insideRoundedSquare(x, y, size)) continue;
          inside += 1;
          if (insideGlyph(x, y, size)) glyph += 1;
        }
      }
      const idx = (py * size + px) * 4;
      if (inside === 0) {
        rgba[idx + 3] = 0;
        continue;
      }
      const mix = glyph / inside;
      rgba[idx] = Math.round(BRAND.r + (WHITE.r - BRAND.r) * mix);
      rgba[idx + 1] = Math.round(BRAND.g + (WHITE.g - BRAND.g) * mix);
      rgba[idx + 2] = Math.round(BRAND.b + (WHITE.b - BRAND.b) * mix);
      rgba[idx + 3] = Math.round((inside / total) * 255);
    }
  }
  return rgba;
}

function drawSolidTile(size) {
  const rgba = Buffer.alloc(size * size * 4);
  for (let i = 0; i < size * size; i += 1) {
    rgba[i * 4] = BRAND.r;
    rgba[i * 4 + 1] = BRAND.g;
    rgba[i * 4 + 2] = BRAND.b;
    rgba[i * 4 + 3] = 255;
  }
  return rgba;
}

// --- entry points -----------------------------------------------------------

export function generateIcons(outDir) {
  mkdirSync(outDir, { recursive: true });
  const written = [];
  for (const size of SIZES) {
    const file = path.join(outDir, "icon-" + size + ".png");
    let tile;
    try {
      tile = drawGlyphTile(size);
    } catch {
      tile = drawSolidTile(size);
    }
    writeFileSync(file, encodePng(size, size, tile));
    written.push({ file, bytes: statSync(file).size });
  }
  return written;
}

function main() {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const outDir = process.argv[2] ? path.resolve(process.argv[2]) : here;
  for (const item of generateIcons(outDir)) {
    console.log("wrote " + item.file + " (" + item.bytes + " bytes)");
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
