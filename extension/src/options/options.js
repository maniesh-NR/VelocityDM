// SPDX-License-Identifier: GPL-3.0-or-later
// VelocityDM Companion options page: connection, sniffing, behavior.

const CATEGORY_KEYS = ["video", "audio", "archive", "document", "software", "image-large", "other"];

function el(id) {
  return document.getElementById(id);
}

function send(message) {
  return chrome.runtime.sendMessage(message).catch((error) => ({ ok: false, error: String(error) }));
}

function buildCategoryInputs(settings) {
  const box = el("categories");
  box.textContent = "";
  for (const key of CATEGORY_KEYS) {
    const label = document.createElement("label");
    const input = document.createElement("input");
    input.type = "checkbox";
    input.dataset.category = key;
    input.checked = Boolean(settings.categories[key]);
    label.append(input, document.createTextNode(key));
    box.appendChild(label);
  }
}

async function load() {
  const state = await send({ type: "getState" });
  const settings = state && state.ok ? state.settings : null;
  if (!settings) return;
  el("host").value = settings.host;
  el("port").value = settings.port;
  el("minSizeMB").value = settings.minSizeMB;
  el("sniffEnabled").checked = settings.sniffEnabled;
  el("contextMenu").checked = settings.contextMenu;
  el("notify").checked = settings.notify;
  el("blocklist").value = settings.blocklist.join("\n");
  buildCategoryInputs(settings);
}

async function save() {
  const patch = {
    host: el("host").value.trim(),
    port: Number(el("port").value),
    minSizeMB: Number(el("minSizeMB").value),
    sniffEnabled: el("sniffEnabled").checked,
    contextMenu: el("contextMenu").checked,
    notify: el("notify").checked,
    blocklist: el("blocklist").value
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean),
    categories: {},
  };
  for (const input of el("categories").querySelectorAll("input[data-category]")) {
    patch.categories[input.dataset.category] = input.checked;
  }
  // Persist directly: the background's storage.onChanged listener picks the
  // change up, invalidates its cached settings, and refreshes the context menu.
  const stored = await chrome.storage.local.get("settings");
  const merged = { ...(stored.settings || {}), ...patch };
  await chrome.storage.local.set({ settings: merged });
  el("saved").textContent = "saved";
  setTimeout(() => {
    el("saved").textContent = "";
  }, 2500);
}

el("test-connection").addEventListener("click", async () => {
  const resultEl = el("test-result");
  resultEl.classList.remove("error");
  resultEl.textContent = "checking...";
  const result = await send({ type: "testConnection" });
  if (result && result.ok) {
    resultEl.textContent = "connected" + (result.version ? " - v" + result.version : "");
  } else {
    resultEl.classList.add("error");
    resultEl.textContent = result && result.error ? result.error : "daemon not running";
  }
});

el("save").addEventListener("click", () => {
  save().catch(() => {
    el("saved").classList.add("error");
    el("saved").textContent = "save failed";
  });
});

load();
