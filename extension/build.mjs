// SPDX-License-Identifier: GPL-3.0-or-later
// Build script for VelocityDM Companion.
//
// Assembles per-browser packages into dist/ and zips them:
//   dist/chrome/   (Chromium base manifest)
//   dist/edge/     (same as chrome, description prefixed "Edge - ")
//   dist/opera/    (same as chrome)
//   dist/firefox/  (Firefox overlay manifest + shared src/)
// Run with: bun build.mjs

import { cpSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { generateIcons } from "./src/icons/make-icons.mjs";

const root = path.dirname(fileURLToPath(import.meta.url));
const distDir = path.join(root, "dist");
const srcDir = path.join(root, "src");

function readBaseManifest() {
  return JSON.parse(readFileSync(path.join(root, "manifest.json"), "utf8"));
}

function countFiles(dir) {
  let count = 0;
  const walk = (current) => {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      if (entry.isDirectory()) walk(path.join(current, entry.name));
      else count += 1;
    }
  };
  walk(dir);
  return count;
}

function packageZip(name, version, dir) {
  const zipPath = path.join(distDir, "velocitydm-" + name + "-" + version + ".zip");
  execFileSync("zip", ["-r", "-q", zipPath, "."], { cwd: dir, stdio: "pipe" });
  return zipPath;
}

function main() {
  const baseManifest = readBaseManifest();
  const version = baseManifest.version;

  // Icons are generated first so every package copy contains them.
  const icons = generateIcons(path.join(srcDir, "icons"));
  console.log("icons: " + icons.map((item) => path.basename(item.file) + " " + item.bytes + "B").join(", "));

  rmSync(distDir, { recursive: true, force: true });
  mkdirSync(distDir, { recursive: true });

  const chromiumTargets = [
    { name: "chrome", manifest: baseManifest },
    { name: "edge", manifest: { ...baseManifest, description: "Edge - " + baseManifest.description } },
    { name: "opera", manifest: baseManifest },
  ];

  for (const target of chromiumTargets) {
    const dir = path.join(distDir, target.name);
    mkdirSync(dir, { recursive: true });
    cpSync(srcDir, path.join(dir, "src"), { recursive: true });
    writeFileSync(path.join(dir, "manifest.json"), JSON.stringify(target.manifest, null, 2) + "\n");
  }

  const firefoxDir = path.join(distDir, "firefox");
  mkdirSync(firefoxDir, { recursive: true });
  cpSync(srcDir, path.join(firefoxDir, "src"), { recursive: true });
  cpSync(path.join(root, "firefox", "manifest.json"), path.join(firefoxDir, "manifest.json"));

  const zips = [];
  for (const name of ["chrome", "edge", "opera", "firefox"]) {
    zips.push({ name, zip: packageZip(name, version, path.join(distDir, name)) });
  }

  console.log("VelocityDM Companion build " + version);
  for (const item of zips) {
    const bytes = statSync(item.zip).size;
    const files = countFiles(path.join(distDir, item.name));
    console.log(
      "  " + path.basename(item.zip).padEnd(32) + String(bytes).padStart(10) + " bytes  (" + files + " files)"
    );
  }
}

main();
