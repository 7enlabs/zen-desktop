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
