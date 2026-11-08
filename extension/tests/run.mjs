// SPDX-License-Identifier: GPL-3.0-or-later
// Test runner for VelocityDM Companion. Imports every test module (each one
// registers cases through the shared collector), runs them sequentially, and
// exits non-zero when any case fails. Run with: bun tests/run.mjs

import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));

const registered = [];
function test(name, fn) {
  registered.push({ name, fn });
}

const modules = [
  "./classify.test.mjs",
  "./protocol.test.mjs",
  "./settings.test.mjs",
  "./dedupe.test.mjs",
  "./sniff.test.mjs",
];

for (const module of modules) {
  const mod = await import(path.join(here, module));
  if (typeof mod.default !== "function") {
    console.error("FAIL " + module + " does not default-export register(test)");
    process.exit(1);
  }
  mod.default(test);
}

let passed = 0;
let failed = 0;
for (const { name, fn } of registered) {
  try {
    await fn();
    passed += 1;
    console.log("PASS " + name);
  } catch (error) {
    failed += 1;
    console.log("FAIL " + name);
    console.log("     " + String((error && error.message) || error).split("\n")[0]);
  }
}

console.log("");
console.log(passed + "/" + (passed + failed) + " passed");
if (failed > 0) process.exit(1);
