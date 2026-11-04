"use strict";

const SCHEMA_VERSION = 1;
const state = {
  mode: "mock",
  sessions: [],
  activeSession: null,
  expectedSeq: 0,
  pending: [],
  rafScheduled: false,
  assistantEl: null,
  log: [],
  // Live-run tracking for the §9 status + report (G3): reset on every send.
  runStartedAt: null,
  runTimer: null,
  runTools: [],
  runDenied: [],
};

const MOCK_SESSIONS = [
  { id: "2026-10-17-a1b2c3", title: "Refactor rate_gate.rs", preview: "extract quota persistence…", model: "nemotron-3-nano:free", updated: "17:20" },
  { id: "2026-10-17-d4e5f6", title: "Debug checkpoint restore", preview: "auto-checkpoint before restore…", model: "nemotron-3-nano:free", updated: "16:02" },
  { id: "2026-10-16-9a8b7c", title: "Plan mode spike", preview: "PlanGuardedTool intercepts…", model: "nemotron-3-nano:free", updated: "yesterday" },
];

const TAURI = (typeof window !== "undefined" && window.__TAURI__) || null;
const invoke = TAURI ? TAURI.core.invoke : null;
const Channel = TAURI ? TAURI.core.Channel : null;

const LIVE_DEFAULTS = {
  model: "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
  baseUrl: "https://ai-gateway.kilo.ai/api/v1",
  cwd: ".",
};

async function loadSessions() {
  if (!invoke) { state.sessions = MOCK_SESSIONS; return; }
  try {
    const sessions = await invoke("list_sessions");
    state.sessions = sessions.map(s => ({
      id: s.id, title: s.title, preview: s.preview,
      model: s.model, updated: (s.updated_at || "").slice(0, 16).replace("T", " "),
    }));
  } catch (e) {
    console.warn("list_sessions failed, using mock:", e);
    state.sessions = MOCK_SESSIONS;
  }
}

function renderSessions(filter = "") {
  const ul = document.getElementById("session-list");
  ul.innerHTML = "";
  state.sessions
    .filter(s => (s.title + s.preview).toLowerCase().includes(filter.toLowerCase()))
    .forEach(s => {
      const li = document.createElement("li");
      if (state.activeSession === s.id) li.classList.add("active");
      li.innerHTML = `<span class="sess-title"></span><span class="sess-meta"></span>`;
      li.querySelector(".sess-title").textContent = s.title;
      li.querySelector(".sess-meta").textContent = `${s.model} · ${s.updated}`;
      li.dataset.sessionId = s.id;
      li.onclick = () => selectSession(s.id);
      ul.appendChild(li);
    });
}

function selectSession(id) {
  stopRunTimer();
  state.activeSession = id;
  document.getElementById("messages").innerHTML = "";
  state.expectedSeq = 0; state.assistantEl = null;
  hideGap();
  renderSessions(document.getElementById("session-search").value);
  updateShellSession();
  if (typeof timelineSessionChanged === "function") timelineSessionChanged(id);
  if (state.mode === "live") replaySession(id); else playDemo();
}

document.getElementById("tab-chat").onclick = () => showChatTab();
document.getElementById("tab-timeline").onclick = () => showTimelineTab();

document.getElementById("tab-network").onclick = () => showNetworkTab();

document.getElementById("tab-terminal").onclick = () => showTerminalTab();

async function replaySession(id) {
  try {
    const envs = await invoke("get_log_since", { sessionId: id, since: 0 });
    envs.forEach(onEnvelope);
    setStatus("conn", "● live");
  } catch (e) {
    console.warn("get_log_since failed:", e);
    setStatus("conn", "● live (replay failed)");
  }
}

function onEnvelope(env) {
  if (env.v !== SCHEMA_VERSION) { console.warn("unsupported schema v", env.v); return; }
  if (env.seq !== state.expectedSeq) showGap(env.seq);
  state.expectedSeq = Math.max(state.expectedSeq, env.seq + 1);
  state.pending.push(env);
  state.log.push(JSON.stringify(env));
  scheduleFlush();
}

function scheduleFlush() {
  if (state.rafScheduled) return;
  state.rafScheduled = true;
  requestAnimationFrame(() => { state.rafScheduled = false; flushBatch(); });
}

function flushBatch() {
  const batch = state.pending.splice(0);
  for (const env of batch) applyEnvelope(env);
  const box = document.getElementById("messages");
  box.scrollTop = box.scrollHeight;
  updateDiag();
}

function showGap(got) {
  const w = document.getElementById("gap-warning");
  document.getElementById("gap-seqs").textContent = `${state.expectedSeq}..${got - 1}`;
  w.classList.remove("hidden");
}
function hideGap() { document.getElementById("gap-warning").classList.add("hidden"); }
document.getElementById("gap-replay").onclick = async () => {
  if (state.mode === "live" && state.activeSession && invoke) {
    try {
      const envs = await invoke("get_log_since", { sessionId: state.activeSession, since: state.expectedSeq });
      hideGap();
      envs.forEach(onEnvelope);
      return;
    } catch (e) { console.warn("gap replay failed:", e); }
  }

  hideGap(); state.expectedSeq = state.log.length;
};

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function applyEnvelope(env) {
  const box = document.getElementById("messages");
  switch (env.type) {
    case "text_delta": {
      if (!state.assistantEl) {
        state.assistantEl = el("div", "msg assistant typing");
        state.assistantEl.append(el("div", "role", "ZenCode"), el("div", "body"));
        box.append(state.assistantEl);
      }
      state.assistantEl.querySelector(".body").textContent += env.text || "";
      break;
    }
    case "assistant_message": {
      state.assistantEl = null;
      box.append(wrapMsg("assistant", "ZenCode", env.content || ""));
      // Tool-call tree cards (G3, §9 pattern 2): every call the model made,
      // with the files it touches as evidence. Data is real: tool_calls ride
      // on the assistant envelope itself.
      if (Array.isArray(env.tool_calls)) {
        for (const tc of env.tool_calls) {
          const files = filesFromToolArgs(tc.name, tc.arguments);
          state.runTools.push({
            id: tc.id, name: tc.name || "tool", files,
            status: "pending",
          });
          const card = el("details", "msg toolcall");
          const summary = el("summary", "",
            (tc.name || "tool") + (files.length ? " — " + files.join(", ") : ""));
          card.append(summary);
          if (tc.arguments) {
            card.append(el("pre", "", String(tc.arguments).slice(0, 500)));
          }
          box.append(card);
        }
      }
      break;
    }
    case "tool_result": {
      state.assistantEl = null;
      // Engine sends is_error + content (events.rs ToolResult); the
      // demo fixture sends output with no error flag. Accept both.
      const isErr = env.is_error === true || env.error === true;
      const tracked = state.runTools.find(t => t.id === env.tool_use_id);
      if (tracked) tracked.status = isErr ? "error" : "ok";
      const d = el("details", "msg tool");
      const ok = !isErr;
      d.innerHTML = `<summary></summary><pre></pre>`;
      const s = d.querySelector("summary");
      s.append(el("span", ok ? "ok" : "err", ok ? "✓ " : "✗ "), document.createTextNode(`${env.tool_name || "tool"} `));
      s.append(el("span", "", env.tool_use_id ? `· ${env.tool_use_id.slice(0, 8)}` : ""));
      d.querySelector("pre").textContent = ((env.content !== undefined ? env.content : env.output) || "").slice(0, 4000);
      box.append(d);
      break;
    }
    case "permission_denied": {
      // Human-judgment point (G3, §9 pattern 5): labelled as needing a
      // decision, kept separate from machine-fixable items.
      state.runDenied.push(env.tool_name || "tool");
      box.append(wrapMsg("assistant denied", "Needs your decision",
        `Permission denied for ${env.tool_name || "tool"}: ${env.reason || env.text || ""}`));
      break;
    }
    case "system_message":
      box.append(wrapMsg("system", "", env.text || env.event || ""));
      break;
    case "api_retry":
      setStatus("conn", `↻ retry ${env.attempt || ""}`.trim());
      break;
    case "compact_boundary":
      box.append(wrapMsg("system", "", "- context compacted -"));
      break;
    case "quota_state":
      if (typeof RateGateUI !== "undefined") RateGateUI.onEnvelope(env);
      break;
    case "session_end": {
      // Run finished (G3, §9 patterns 3-4): stop the elapsed timer, show
      // real duration + tokens from the envelope, append the structured
      // report with file paths as evidence for every finding.
      stopRunTimer();
      const tokens = fmtTokens(env.input_tokens, env.output_tokens);
      setStatus("conn", `● live · ${fmtDuration(env.duration_ms)} · ${tokens}`);
      appendRunReport(env);
      break;
    }
    default:
      console.warn("unknown envelope type", env.type);
  }
}

function wrapMsg(cls, role, text) {
  const m = el("div", "msg " + cls);
  if (role) m.append(el("div", "role", role));
  const b = el("div", "body", text);
  m.append(b);
  return m;
}

async function sendInput() {
  const input = document.getElementById("input");
  const text = input.value.trim();
  if (!text) return;
  input.value = "";
  hideComposerHints();
  // Fresh run: drop the previous run's tool/decision tracking (G3 report).
  stopRunTimer();
  state.runTools = [];
  state.runDenied = [];
  const box = document.getElementById("messages");
  const m = el("div", "msg user", text);
  box.append(m);
  box.scrollTop = box.scrollHeight;
  if (state.mode === "mock") { mockRespond(text); return; }

  await liveRun(text);
}

async function liveRun(text) {
  const channel = new Channel();
  channel.onmessage = onEnvelope;
  startRunTimer();
  setStatus("conn", "● live · running… 0s");
  try {
    const sessionId = await invoke("run_task", {
      prompt: text,
      model: LIVE_DEFAULTS.model,
      baseUrl: LIVE_DEFAULTS.baseUrl,
      cwd: LIVE_DEFAULTS.cwd,
      onEvent: channel,
    });

    state.sessions.unshift({ id: sessionId, title: text.slice(0, 60), preview: "", model: LIVE_DEFAULTS.model, updated: "now" });
    state.activeSession = sessionId;
    renderSessions(document.getElementById("session-search").value);
  } catch (e) {
    stopRunTimer();
    box_append_error(String(e));
    setStatus("conn", "● live · error");
  }
}

function box_append_error(text) {
  const box = document.getElementById("messages");
  box.append(wrapMsg("assistant denied", "ZenCode", "Error: " + text));
  box.scrollTop = box.scrollHeight;
}

document.getElementById("send").onclick = sendInput;
document.getElementById("input").addEventListener("keydown", e => {
  if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendInput(); }
});
document.getElementById("input").addEventListener("input", updateComposerHints);
document.getElementById("input").addEventListener("focus", updateComposerHints);
document.getElementById("input").addEventListener("blur", hideComposerHints);
document.getElementById("new-session").onclick = () => {
  const id = "draft-" + Date.now().toString(36);
  state.sessions.unshift({ id, title: "New session", preview: "", model: "nemotron-3-nano:free", updated: "now" });
  renderSessions(); selectSession(id);
};
document.getElementById("session-search").addEventListener("input", e => renderSessions(e.target.value));

function setStatus(part, text) {
  const el = document.getElementById("status-" + part);
  if (el) el.textContent = text;
}

// Native confirm dialog (G5): Tauri dialog plugin when running in the app,
// window.confirm fallback in the browser/demo. Returns true when confirmed.
// Verified against tauri-plugin-dialog 2.8.3 source (commands.rs: message
// takes title/message/kind/buttons; kind "warning", buttons "OkCancel"
// deserialize from plain strings; result serializes as "Ok"/"Cancel").
async function confirmNative(title, message) {
  if (invoke) {
    try {
      const res = await invoke("plugin:dialog|message", {
        title,
        message,
        kind: "warning",
        buttons: "OkCancel",
      });
      return res === "Ok";
    } catch (e) {
      console.warn("native dialog failed, falling back to window.confirm:", e);
    }
  }
  return window.confirm(message);
}

// File paths from a tool call's arguments (G3): same key list as
// tlFilesForTool in timeline.js. Shell tools carry commands, not files.
function filesFromToolArgs(name, argsStr) {
  const n = (name || "").toLowerCase();
  if (n.includes("bash") || n.includes("shell") || n.includes("exec")) return [];
  let args;
  try { args = JSON.parse(argsStr || "{}"); } catch { return []; }
  const out = [];
  for (const k of ["path", "file", "file_path", "filePath", "filename"]) {
    const p = args[k];
    if (typeof p === "string" && p.trim() && !out.includes(p.trim())) out.push(p.trim());
  }
  return out;
}

function fmtDuration(ms) {
  if (typeof ms !== "number" || ms < 0) return "unknown time";
  return (ms / 1000).toFixed(1) + "s";
}

function fmtTokens(inputTokens, outputTokens) {
  const input = typeof inputTokens === "number" ? inputTokens : 0;
  const output = typeof outputTokens === "number" ? outputTokens : 0;
  return (input + output) + " tokens (" + input + " in · " + output + " out)";
}

function startRunTimer() {
  stopRunTimer();
  state.runStartedAt = Date.now();
  state.runTimer = setInterval(() => {
    const secs = Math.floor((Date.now() - state.runStartedAt) / 1000);
    setStatus("conn", `● live · running… ${secs}s`);
  }, 1000);
}

function stopRunTimer() {
  if (state.runTimer) { clearInterval(state.runTimer); state.runTimer = null; }
  state.runStartedAt = null;
}

// Structured run report (G3, §9 pattern 4): numbered tool calls with their
// file paths as evidence, points needing a human decision kept separate,
// session id + model + duration + tokens as metadata. Only real data from
// the envelopes of this run; nothing is invented.
function appendRunReport(env) {
  const box = document.getElementById("messages");
  const report = el("div", "msg report");
  const secs = fmtDuration(env.duration_ms);
  const tokens = fmtTokens(env.input_tokens, env.output_tokens);
  const turns = typeof env.num_turns === "number" ? env.num_turns : 0;
  report.append(el("div", "report-head",
    `Run report — ${secs} · ${tokens} · ${turns} turns`));
  if (state.runTools.length) {
    const list = document.createElement("ol");
    for (const t of state.runTools) {
      const mark = t.status === "ok" ? "✓ " : t.status === "error" ? "✗ " : "· ";
      const files = t.files.length ? " — " + t.files.join(", ") : "";
      list.append(el("li", "", mark + t.name + files));
    }
    report.append(list);
  } else {
    report.append(el("div", "report-files", "Files: none recorded"));
  }
  if (state.runDenied.length) {
    report.append(el("div", "report-decision",
      "Needs your decision: " + [...new Set(state.runDenied)].join(", ")));
  }
  const meta = [];
  if (state.activeSession) meta.push("Session: " + state.activeSession);
  if (env.model) meta.push("Model: " + env.model);
  if (meta.length) report.append(el("div", "report-meta", meta.join(" · ")));
  box.append(report);
  box.scrollTop = box.scrollHeight;
}

// Composer hints (G3, §9 pattern 1): key hints at the place of typing.
// The app and the engine have no slash commands (verified by grep on
// 05/11/2026), so no slash list is invented; typing "/" says so plainly.
function updateComposerHints() {
  const input = document.getElementById("input");
  const hints = document.getElementById("composer-hints");
  if (!input || !hints) return;
  if (input.value.startsWith("/")) {
    hints.textContent = "Slash commands are not available in this build — type your task plainly.";
  } else {
    hints.textContent = "Enter to send · Shift+Enter for a new line";
  }
  hints.classList.remove("hidden");
}

function hideComposerHints() {
  const hints = document.getElementById("composer-hints");
  if (hints) hints.classList.add("hidden");
}

let demoTimers = [];

let quotaDemoTimers = [];
function demoQuotaCycle() {
  quotaDemoTimers.forEach(clearTimeout); quotaDemoTimers = [];
  if (typeof RateGateUI === "undefined" || !RateGateUI.init(document)) return;
  const order = ["healthy", "approaching", "at_limit", "cooldown", "healthy"];
  order.forEach((name, i) => {
    quotaDemoTimers.push(setTimeout(() => {
      RateGateUI.update(RateGateUI.MOCK_STATES[name]);
    }, i * 4000));
  });
}

async function playDemo() {
  demoTimers.forEach(clearTimeout); demoTimers = [];
  setStatus("model", "model: nemotron-3-nano:free");
  demoQuotaCycle();
  setStatus("conn", "● mock");
  let lines;
  try {
    const res = await fetch("demo.ndjson");
    lines = (await res.text()).trim().split("\n");
  } catch { return; }
  let delay = 400, seq = 0;
  for (const line of lines) {
    const env = JSON.parse(line);
    env.seq = seq++;
    const isDelta = env.type === "text_delta";
    delay += isDelta ? 60 : 500;
    demoTimers.push(setTimeout(() => onEnvelope(env), delay));
  }
}

function mockRespond(text) {
  const base = state.expectedSeq;
  const reply = [
    { v: 1, type: "text_delta", text: "Mock mode - no engine connected yet. " },
    { v: 1, type: "text_delta", text: `You said: "${text.slice(0, 60)}". ` },
    { v: 1, type: "assistant_message", content: `Mock mode - no engine connected yet. You said: "${text.slice(0, 60)}".` },
  ];
  reply.forEach((env, i) => { env.seq = base + i; setTimeout(() => onEnvelope(env), 300 * (i + 1)); });
}


/* Window shell added by the desktop design-audit fix: native menu events,
   in-window menu bar, command palette, context menu, toolbar, inspector,
   collapsible sidebar, density toggle, per-session window title. */

const DENSITY_STATES = ["normal", "compact", "comfortable"];
const DENSITY_LABELS = { normal: "Normal", compact: "Compact", comfortable: "Comfortable" };
let densityIndex = 0;
let paletteItems = [];
let paletteIndex = 0;

function shellStore(key, value) { try { localStorage.setItem(key, value); } catch (e) {} }
function shellRecall(key) { try { return localStorage.getItem(key); } catch (e) { return null; } }
function clickById(id) { const el = document.getElementById(id); if (el) el.click(); }
function focusById(id) { const el = document.getElementById(id); if (el) el.focus(); }

function copyText(text) {
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).catch(() => fallbackCopy(text));
  } else { fallbackCopy(text); }
}
function fallbackCopy(text) {
  const ta = document.createElement("textarea");
  ta.value = text;
  document.body.appendChild(ta);
  ta.select();
  try { document.execCommand("copy"); } catch (e) {}
  ta.remove();
}

function activeSessionObj() {
  return state.sessions.find(s => s.id === state.activeSession) || null;
}

function updateWindowTitle() {
  const s = activeSessionObj();
  const title = s ? s.title + " \u2014 ZenCode Desktop" : "ZenCode Desktop";
  document.title = title;
  try {
    const w = TAURI && window.__TAURI__ && window.__TAURI__.window;
    if (w && typeof w.getCurrentWindow === "function") {
      const win = w.getCurrentWindow();
      if (win && typeof win.setTitle === "function") {
        const p = win.setTitle(title);
        if (p && typeof p.catch === "function") p.catch(() => {});
      }
    }
  } catch (e) {}
}

function updateDiag() {
  const el = document.getElementById("status-diag");
  if (el) el.textContent = "envelopes: " + state.log.length + " \u00b7 seq: " + state.expectedSeq;
}

function updateShellSession() {
  updateWindowTitle();
  updateDiag();
  const label = document.getElementById("tb-session");
  const s = activeSessionObj();
  if (label) label.textContent = s ? s.title : "No session";
  const panel = document.getElementById("inspector");
  if (panel && !panel.classList.contains("hidden")) renderInspector();
}

function setSidebarCollapsed(collapsed) {
  document.getElementById("app").classList.toggle("sidebar-collapsed", collapsed);
  const btn = document.getElementById("tb-sidebar");
  if (btn) btn.setAttribute("aria-pressed", collapsed ? "true" : "false");
  shellStore("zd-sidebar-collapsed", collapsed ? "1" : "0");
}
function toggleSidebar() {
  setSidebarCollapsed(!document.getElementById("app").classList.contains("sidebar-collapsed"));
}

function setDensity(index) {
  densityIndex = ((index % 3) + 3) % 3;
  const name = DENSITY_STATES[densityIndex];
  document.body.dataset.density = name;
  const btn = document.getElementById("tb-density");
  if (btn) btn.textContent = "Density: " + DENSITY_LABELS[name];
  shellStore("zd-density", name);
}
function toggleDensity() { setDensity(densityIndex + 1); }

function cycleSession(dir) {
  if (!state.sessions.length) return;
  const idx = state.sessions.findIndex(s => s.id === state.activeSession);
  const next = (idx + dir + state.sessions.length) % state.sessions.length;
  selectSession(state.sessions[next].id);
}

function renderInspector() {
  const box = document.getElementById("inspector-body");
  if (!box) return;
  const s = activeSessionObj();
  const rows = [
    ["Session", s ? s.title + " (" + s.id + ")" : "none"],
    ["Model", s ? s.model : "-"],
    ["Mode", state.mode],
    ["Workspace", state.mode === "live" ? LIVE_DEFAULTS.cwd : "mock demo (no engine)"],
    ["Scope", state.mode === "live" ? "working directory only" : "not applicable (mock mode)"],
    ["Envelopes", String(state.log.length)],
    ["Expected seq", String(state.expectedSeq)],
    ["Pending", String(state.pending.length)],
  ];
  box.innerHTML = "";
  rows.forEach(r => {
    const dt = document.createElement("dt");
    dt.textContent = r[0];
    const dd = document.createElement("dd");
    dd.textContent = r[1];
    box.append(dt, dd);
  });
}
function toggleInspector() {
  const panel = document.getElementById("inspector");
  if (!panel) return;
  panel.classList.toggle("hidden");
  if (!panel.classList.contains("hidden")) renderInspector();
}

function showAbout() {
  const overlay = document.getElementById("about-overlay");
  const body = document.getElementById("about-body");
  const s = activeSessionObj();
  if (body) body.textContent = "ZenCode Desktop \u00b7 mode: " + state.mode + " \u00b7 session: " + (s ? s.title : "none");
  if (overlay) overlay.classList.remove("hidden");
}
function hideAbout() {
  const overlay = document.getElementById("about-overlay");
  if (overlay) overlay.classList.add("hidden");
}

function closeMenus() {
  document.querySelectorAll("#menubar .menu-drop").forEach(d => d.classList.add("hidden"));
}
function closePalette() {
  const overlay = document.getElementById("palette");
  if (overlay) overlay.classList.add("hidden");
}
function openPalette() {
  const overlay = document.getElementById("palette");
  if (!overlay) return;
  overlay.classList.remove("hidden");
  const input = document.getElementById("palette-input");
  input.value = "";
  paletteIndex = 0;
  renderPalette("");
  input.focus();
}
function renderPalette(filter) {
  const list = document.getElementById("palette-list");
  const q = filter.toLowerCase();
  paletteItems = COMMANDS.filter(c => (c.label + " " + c.id).toLowerCase().includes(q));
  if (paletteIndex >= paletteItems.length) paletteIndex = Math.max(0, paletteItems.length - 1);
  list.innerHTML = "";
  paletteItems.forEach((c, i) => {
    const li = document.createElement("li");
    li.dataset.cmd = c.id;
    if (i === paletteIndex) li.classList.add("selected");
    const name = document.createElement("span");
    name.textContent = c.label;
    const hint = document.createElement("span");
    hint.className = "accel";
    hint.textContent = c.hint;
    li.append(name, hint);
    list.appendChild(li);
  });
}

const COMMANDS = [
  { id: "new-session", label: "New Session", hint: "Ctrl/Cmd+N", run: () => clickById("new-session") },
  { id: "open-palette", label: "Command Palette", hint: "Ctrl/Cmd+K", run: openPalette },
  { id: "focus-composer", label: "Focus Composer", hint: "Ctrl/Cmd+L", run: () => focusById("input") },
  { id: "focus-search", label: "Search Sessions", hint: "/", run: () => focusById("session-search") },
  { id: "toggle-sidebar", label: "Toggle Sidebar", hint: "Ctrl/Cmd+B", run: toggleSidebar },
  { id: "toggle-density", label: "Toggle Density", hint: "Ctrl/Cmd+Shift+D", run: toggleDensity },
  { id: "toggle-inspector", label: "Toggle Inspector", hint: "", run: toggleInspector },
  { id: "tab-chat", label: "Show Chat Tab", hint: "Ctrl/Cmd+1", run: () => clickById("tab-chat") },
  { id: "tab-timeline", label: "Show Timeline Tab", hint: "Ctrl/Cmd+2", run: () => clickById("tab-timeline") },
  { id: "tab-network", label: "Show Network Tab", hint: "Ctrl/Cmd+3", run: () => clickById("tab-network") },
  { id: "tab-terminal", label: "Show Terminal Tab", hint: "Ctrl/Cmd+4", run: () => clickById("tab-terminal") },
  { id: "next-session", label: "Next Session", hint: "", run: () => cycleSession(1) },
  { id: "prev-session", label: "Previous Session", hint: "", run: () => cycleSession(-1) },
  { id: "copy-session-id", label: "Copy Session ID", hint: "", run: () => { if (state.activeSession) copyText(state.activeSession); } },
  { id: "about", label: "About ZenCode Desktop", hint: "", run: showAbout },
];

function runCommand(id) {
  closeMenus();
  closePalette();
  const cmd = COMMANDS.find(c => c.id === id);
  if (cmd) cmd.run();
}

function hideCtxMenu() {
  const menu = document.getElementById("ctxmenu");
  if (menu) menu.classList.add("hidden");
}
function showCtxMenu(x, y, items) {
  const menu = document.getElementById("ctxmenu");
  const ul = document.getElementById("ctxmenu-list");
  ul.innerHTML = "";
  items.forEach(it => {
    const li = document.createElement("li");
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = it.label;
    btn.addEventListener("click", () => { hideCtxMenu(); it.run(); });
    li.appendChild(btn);
    ul.appendChild(li);
  });
  menu.style.left = x + "px";
  menu.style.top = y + "px";
  menu.classList.remove("hidden");
}

document.querySelectorAll("#menubar .menu-label").forEach(btn => {
  btn.addEventListener("click", e => {
    e.stopPropagation();
    const drop = btn.parentElement.querySelector(".menu-drop");
    const wasHidden = drop.classList.contains("hidden");
    closeMenus();
    if (wasHidden) drop.classList.remove("hidden");
  });
});
document.addEventListener("click", e => {
  if (!e.target.closest("#menubar")) closeMenus();
  if (!e.target.closest("#ctxmenu")) hideCtxMenu();
  const cmdBtn = e.target.closest("[data-cmd]");
  if (cmdBtn) runCommand(cmdBtn.dataset.cmd);
});
document.addEventListener("contextmenu", e => {
  if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA") return;
  const msg = e.target.closest(".msg");
  const sessLi = e.target.closest("#session-list li");
  let items;
  if (msg) {
    const text = msg.textContent;
    items = [
      { label: "Copy Message Text", run: () => copyText(text) },
      { label: "Copy Session ID", run: () => { if (state.activeSession) copyText(state.activeSession); } },
    ];
  } else if (sessLi && sessLi.dataset.sessionId) {
    const sid = sessLi.dataset.sessionId;
    items = [
      { label: "Open Session", run: () => selectSession(sid) },
      { label: "Copy Session ID", run: () => copyText(sid) },
    ];
  } else {
    items = [
      { label: "Command Palette", run: openPalette },
      { label: "New Session", run: () => clickById("new-session") },
    ];
  }
  e.preventDefault();
  showCtxMenu(e.clientX, e.clientY, items);
});
document.addEventListener("keydown", e => {
  const mod = e.metaKey || e.ctrlKey;
  const typing = e.target && (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA");
  if (e.key === "Escape") { closeMenus(); closePalette(); hideCtxMenu(); hideAbout(); return; }
  if (mod && e.key.toLowerCase() === "k") { e.preventDefault(); openPalette(); return; }
  if (mod && e.key.toLowerCase() === "n") { e.preventDefault(); runCommand("new-session"); return; }
  if (mod && e.key.toLowerCase() === "b") { e.preventDefault(); toggleSidebar(); return; }
  if (mod && e.key.toLowerCase() === "l") { e.preventDefault(); focusById("input"); return; }
  if (mod && e.shiftKey && e.key.toLowerCase() === "d") { e.preventDefault(); toggleDensity(); return; }
  if (mod && ["1", "2", "3", "4"].includes(e.key)) {
    e.preventDefault();
    runCommand(["tab-chat", "tab-timeline", "tab-network", "tab-terminal"][Number(e.key) - 1]);
    return;
  }
  if (!mod && !typing && e.key === "/") { e.preventDefault(); focusById("session-search"); }
});
document.getElementById("palette-input").addEventListener("keydown", e => {
  if (e.key === "ArrowDown") { e.preventDefault(); paletteIndex = Math.min(paletteIndex + 1, Math.max(0, paletteItems.length - 1)); renderPalette(e.target.value); }
  else if (e.key === "ArrowUp") { e.preventDefault(); paletteIndex = Math.max(0, paletteIndex - 1); renderPalette(e.target.value); }
  else if (e.key === "Enter") { e.preventDefault(); const c = paletteItems[paletteIndex]; if (c) runCommand(c.id); }
});
document.getElementById("palette-input").addEventListener("input", e => { paletteIndex = 0; renderPalette(e.target.value); });

(function initShell() {
  setSidebarCollapsed(shellRecall("zd-sidebar-collapsed") === "1");
  const savedDensity = shellRecall("zd-density");
  setDensity(Math.max(0, DENSITY_STATES.indexOf(savedDensity)));
  const aboutClose = document.getElementById("about-close");
  if (aboutClose) aboutClose.addEventListener("click", hideAbout);
  const aboutOverlay = document.getElementById("about-overlay");
  if (aboutOverlay) aboutOverlay.addEventListener("click", e => { if (e.target === aboutOverlay) hideAbout(); });
  const paletteOverlay = document.getElementById("palette");
  if (paletteOverlay) paletteOverlay.addEventListener("click", e => { if (e.target === paletteOverlay) closePalette(); });
  try {
    const ev = TAURI && window.__TAURI__ && window.__TAURI__.event;
    if (ev && typeof ev.listen === "function") {
      ev.listen("menu-command", e => { if (typeof e.payload === "string") runCommand(e.payload); });
    }
  } catch (err) {}
})();

(async function boot() {
  if (TAURI && invoke) {
    state.mode = "live";
    setStatus("conn", "● live");
    setStatus("model", "model: " + LIVE_DEFAULTS.model);
    await loadSessions();
  } else {
    state.sessions = MOCK_SESSIONS;
  }
  renderSessions();
  if (state.sessions.length) selectSession(state.sessions[0].id);

  try {
    if (typeof maybeStartOnboarding === "function" && typeof localStorage !== "undefined") {
      maybeStartOnboarding({
        document,
        storage: localStorage,
        onFinish: (result) => {
          if (result.completed && result.modelId) {
            setStatus("model", "model: " + result.modelId);
          }
        },
      });
    }
  } catch (e) {
    console.warn("onboarding failed to start:", e);
  }
})();
