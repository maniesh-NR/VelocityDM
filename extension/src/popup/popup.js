// SPDX-License-Identifier: GPL-3.0-or-later
// VelocityDM Companion popup: daemon status, manual add, captured list.

function formatSize(bytes) {
  if (typeof bytes !== "number" || !Number.isFinite(bytes) || bytes < 0) return "";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < 4) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? bytes + " B" : value.toFixed(1) + " KBMBGT"[unit];
}

function el(id) {
  return document.getElementById(id);
}

function send(message) {
  return chrome.runtime.sendMessage(message).catch((error) => ({ ok: false, error: String(error) }));
}

function renderCaptures(captures) {
  const list = el("capture-list");
  list.textContent = "";
  if (!captures.length) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "No captured downloads yet. Browse a video or file page with the daemon running.";
    list.appendChild(empty);
    return;
  }
  for (const cap of captures) {
    const row = document.createElement("div");
    row.className = "item";
    const chip = document.createElement("span");
    chip.className = "chip";
    chip.textContent = cap.category || "other";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = cap.filename || cap.url;
    name.title = cap.url;
    const size = document.createElement("span");
    size.className = "size";
    size.textContent = formatSize(cap.size);
    const button = document.createElement("button");
    button.textContent = "Download";
    button.addEventListener("click", async () => {
      button.disabled = true;
      await send({ type: "addUrl", url: cap.url });
      button.disabled = false;
    });
    row.append(chip, name, size, button);
    list.appendChild(row);
  }
}

async function refresh() {
  const state = await send({ type: "getState" });
  const dot = el("daemon-dot");
  const status = el("daemon-status");
  if (state && state.ok && state.connected) {
    dot.className = "dot ok";
    status.textContent = "daemon connected" + (state.version ? " - v" + state.version : "");
  } else {
    dot.className = "dot bad";
    status.textContent = "daemon not running";
  }
  const caps = await send({ type: "getCaptures" });
  renderCaptures(caps && caps.ok ? caps.captures : []);
}

el("add-btn").addEventListener("click", async () => {
  const input = el("add-url");
  const url = input.value.trim();
  if (!url) return;
  input.value = "";
  await send({ type: "addUrl", url });
  await refresh();
});

el("add-url").addEventListener("keydown", (event) => {
  if (event.key === "Enter") el("add-btn").click();
});

el("download-all").addEventListener("click", async () => {
  const caps = await send({ type: "getCaptures" });
  if (!caps || !caps.ok) return;
  for (const cap of caps.captures) {
    await send({ type: "addUrl", url: cap.url });
  }
  await refresh();
});

el("clear").addEventListener("click", async () => {
  await send({ type: "clearCaptures" });
  await refresh();
});

el("open-options").addEventListener("click", () => {
  chrome.runtime.openOptionsPage();
});

refresh();
