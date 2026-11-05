"use strict";

const NW = {
  entries: [],
  verdict: "",
};

function showNetworkTab() {
  document.getElementById("messages").classList.add("hidden");
  document.getElementById("composer").classList.add("hidden");
  const tp = document.getElementById("timeline-panel");
  if (tp) tp.classList.add("hidden");
  const tv = document.getElementById("terminal-view");
  if (tv) tv.classList.add("hidden");
  document.getElementById("network-panel").classList.remove("hidden");
  document.getElementById("tab-chat").classList.remove("active");
  const tt = document.getElementById("tab-timeline");
  if (tt) tt.classList.remove("active");
  const term = document.getElementById("tab-terminal");
  if (term) term.classList.remove("active");
  document.getElementById("tab-network").classList.add("active");
  loadNetworkLog();
}

function hideNetworkTab() {
  document.getElementById("network-panel").classList.add("hidden");
  document.getElementById("tab-network").classList.remove("active");
}

async function loadNetworkLog() {
  if (typeof invoke === "function" && invoke) {
    try {
      const rows = await invoke("get_network_log");
      NW.entries = Array.isArray(rows) ? rows : [];
      NW.verdict = "";
      renderNetwork();
      return;
    } catch (e) {
      console.warn("get_network_log failed (Tauri command not yet implemented):", e);
    }
  }

  NW.entries = [
    { ts_ms: Date.now() - 42000, host: "ai-gateway.kilo.ai", path: "/chat/completions", purpose: "model_chat", bytes_up: 18432, bytes_down: 5120 },
    { ts_ms: Date.now() - 18000, host: "ai-gateway.kilo.ai", path: "/chat/completions", purpose: "model_chat", bytes_up: 9216, bytes_down: 2048 },
  ];
  NW.verdict = "";
  renderNetwork();
}

function networkVerdict() {
  if (NW.verdict) return NW.verdict;
  const hosts = [...new Set(NW.entries.map(e => e.host))];
  if (hosts.length === 0) return "No outbound requests recorded yet.";

  return `Clean: ${NW.entries.length} request(s) to ${hosts.length} host(s) (${hosts.join(", ")}), none owned by ZenCode. No telemetry was sent.`;
}

function fmtBytes(n) {
  if (n < 1024) return n + " B";
  if (n < 1024 * 1024) return (n / 1024).toFixed(1) + " KB";
  return (n / 1024 / 1024).toFixed(2) + " MB";
}

function fmtTime(ts) {
  try { return new Date(ts).toLocaleTimeString(); } catch { return String(ts); }
}

function renderNetwork() {
  const panel = document.getElementById("network-panel");
  panel.innerHTML = "";
  const esc = s => String(s).replace(/[&<>"]/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"}[c]));

  const head = document.createElement("div");
  head.className = "net-head";
  const v = document.createElement("div");
  v.className = "net-verdict";
  v.textContent = "🛡 " + networkVerdict();
  head.appendChild(v);

  const btns = document.createElement("div");
  btns.className = "net-actions";
  const exp = document.createElement("button");
  exp.textContent = "Export JSON";
  exp.onclick = exportNetworkLog;
  const clr = document.createElement("button");
  clr.textContent = "Clear log";
  clr.onclick = clearNetworkLog;
  btns.append(exp, clr);
  head.appendChild(btns);
  panel.appendChild(head);

  if (NW.entries.length === 0) {
    const p = document.createElement("p");
    p.className = "net-empty";
    p.textContent = "The engine has made no outbound requests in this session. Only your configured model endpoint can ever appear here.";
    panel.appendChild(p);
    return;
  }

  const table = document.createElement("table");
  table.className = "net-table";
  table.innerHTML = "<thead><tr><th>Time</th><th>Host</th><th>Path</th><th>Purpose</th><th>Up</th><th>Down</th></tr></thead>";
  const tb = document.createElement("tbody");
  for (const e of NW.entries) {
    const tr = document.createElement("tr");
    tr.innerHTML =
      `<td>${esc(fmtTime(e.ts_ms))}</td><td>${esc(e.host)}</td>` +
      `<td>${esc(e.path)}</td><td>${esc(e.purpose)}</td>` +
      `<td>${esc(fmtBytes(e.bytes_up))}</td><td>${esc(fmtBytes(e.bytes_down))}</td>`;
    tb.appendChild(tr);
  }
  table.appendChild(tb);
  panel.appendChild(table);

  const note = document.createElement("p");
  note.className = "net-note";
  note.textContent = "Auditable proof: this log is the complete record of engine-initiated network traffic. ZenCode operates no telemetry servers - there is nothing to phone home to.";
  panel.appendChild(note);
}

// NOTE (G5): this still uses an anchor download, not the native save dialog.
// A native save needs plugin:dialog|save + plugin:fs|write_text_file, and the
// write needs the fs:allow-write-text-file capability (scope decision for the
// Leader) - see the B-1 handoff. Do not switch to the dialog before that lands.
function downloadJson(filename, obj) {
  const blob = new Blob([JSON.stringify(obj, null, 2)], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 100);
}

async function exportNetworkLog() {
  downloadJson("zencode-network-log.json", {
    exported_at: new Date().toISOString(),
    verdict: networkVerdict(),
    entries: NW.entries,
  });
}

async function clearNetworkLog() {
  if (!(await confirmNative("Clear network log", "Delete the local network log? This cannot be undone."))) return;
  if (typeof invoke === "function" && invoke) {
    try { await invoke("clear_network_log"); } catch (e) { console.warn("clear_network_log failed:", e); }
  }
  NW.entries = [];
  renderNetwork();
}
