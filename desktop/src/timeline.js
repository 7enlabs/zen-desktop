"use strict";

const TL = {
  entries: [],
  checkpoints: [],
  sessionId: null,
};

const TAURI_TL = (typeof window !== "undefined" && window.__TAURI__) || null;
const invokeTl = TAURI_TL ? TAURI_TL.core.invoke : null;

function tlKindForTool(name) {
  const n = (name || "").toLowerCase();
  if (n.includes("write")) return "write";
  if (n.includes("edit")) return "edit";
  if (n.includes("bash") || n.includes("shell") || n.includes("exec")) return "bash";
  if (n.includes("read")) return "read";
  return "tool";
}

function tlFilesForTool(name, argsStr) {
  if (tlKindForTool(name) === "bash") return [];
  let args;
  try { args = JSON.parse(argsStr || "{}"); } catch { return []; }
  const out = [];
  for (const k of ["path", "file", "file_path", "filePath", "filename"]) {
    const p = args[k];
    if (typeof p === "string" && p.trim() && !out.includes(p.trim())) out.push(p.trim());
  }
  return out;
}

function tlFirstLine(s, max) {
  const line = String(s || "").split("\n")[0].trim();
  return line.length > max ? line.slice(0, max - 1) + "…" : line;
}

function tlTitleForTool(name, files, argsStr) {
  const file = files.length ? "`" + files[0] + "`" : "";
  switch (tlKindForTool(name)) {
    case "write": return ("Wrote " + file).trim();
    case "edit": return ("Edited " + file).trim();
    case "read": return ("Read " + file).trim();
    case "bash": {
      let cmd = "";
      try {
        const a = JSON.parse(argsStr || "{}");
        cmd = a.command || a.cmd || "";
      } catch {  }
      const short = tlFirstLine(cmd, 60);
      return short ? "Ran `" + short + "`" : "Ran a shell command";
    }
    default: return "Used " + name;
  }
}

function tlParseCheckpointNote(text) {
  const m = /^checkpoint ([0-9a-f]+) before seq (\d+)\s*$/.exec(String(text || "").trim());
  return m ? { id: m[1], seq: Number(m[2]) } : null;
}

function tlBuildFromEnvelopes(envs) {
  const cpForSeq = new Map();
  for (const e of envs) {
    if (e.type === "system_message" && e.event === "checkpoint") {
      const p = tlParseCheckpointNote(e.text);
      if (p) cpForSeq.set(p.seq, p.id);
    }
  }
  const entries = [];
  const byToolUse = new Map();
  let step = 0;
  for (const e of envs) {
    if (e.type === "assistant_message" && Array.isArray(e.tool_calls)) {
      for (const tc of e.tool_calls) {
        step += 1;
        const kind = tlKindForTool(tc.name);
        const files = tlFilesForTool(tc.name, tc.arguments);
        const cp = cpForSeq.get(e.seq) || null;
        entries.push({
          step, seq: e.seq, kind,
          title: tlTitleForTool(tc.name, files, tc.arguments),
          detail: "no result recorded",
          files, checkpoint_id: cp, undoable: !!cp, status: "pending",
        });
        byToolUse.set(tc.id, entries.length - 1);
      }
    } else if (e.type === "tool_result") {
      const idx = byToolUse.get(e.tool_use_id);
      if (idx !== undefined) {
        entries[idx].status = e.is_error ? "error" : "ok";
        entries[idx].detail = tlFirstLine(e.content, 140);
      }
    } else if (e.type === "permission_denied") {
      step += 1;
      entries.push({
        step, seq: e.seq, kind: "blocked",
        title: "Blocked: " + (e.tool_name || "tool") + " needed your approval",
        detail: "the agent did not run this - waiting on you",
        files: [], checkpoint_id: null, undoable: false, status: "blocked",
      });
    } else if (e.type === "system_message" && (e.event === "session_start" || e.event === "session_end")) {
      step += 1;
      entries.push({
        step, seq: e.seq, kind: "system",
        title: e.event === "session_start" ? "Session started" : "Session ended",
        detail: tlFirstLine(e.text, 140),
        files: [], checkpoint_id: null, undoable: false, status: "info",
      });
    }
  }
  return entries;
}

async function loadTimeline(sessionId) {
  TL.sessionId = sessionId;
  if (invokeTl) {
    try {
      const res = await invokeTl("get_timeline", { sessionId });
      TL.entries = res.entries || [];
      TL.checkpoints = res.checkpoints || [];
      renderTimeline();
      return;
    } catch (e) {
      console.warn("get_timeline failed, using demo:", e);
    }
  }

  try {
    const res = await fetch("timeline-demo.ndjson");
    const lines = (await res.text()).trim().split("\n").filter(Boolean);
    const envs = lines.map(l => JSON.parse(l));
    TL.entries = tlBuildFromEnvelopes(envs);

    TL.checkpoints = [
      { id: "e4f5a6b", message: "auto: before Write src/lib.rs", timestamp: 1760692800 },
      { id: "a1b2c3d", message: "auto: before Edit src/main.rs", timestamp: 1760692700 },
    ];
  } catch (e) {
    console.warn("timeline demo failed:", e);
    TL.entries = []; TL.checkpoints = [];
  }
  renderTimeline();
}

function tlEl(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

const KIND_ICON = { write: "✎", edit: "🔧", bash: "⌨", read: "👁", tool: "⚙", blocked: "⛔", system: "ℹ" };
const KIND_LABEL = { write: "write", edit: "edit", bash: "shell", read: "read", tool: "tool", blocked: "blocked", system: "system" };

function renderTimeline() {
  const panel = document.getElementById("timeline-panel");
  panel.innerHTML = "";
  panel.append(tlEl("div", "tl-head", "Time-Travel - every action the agent took, undoable"));

  if (!TL.entries.length) {
    panel.append(tlEl("div", "tl-empty", "No actions recorded for this session yet."));
  }

  const list = tlEl("ol", "tl-list");
  TL.entries.forEach((en, i) => {
    const li = tlEl("li", "tl-entry kind-" + en.kind + " status-" + en.status);
    const icon = tlEl("span", "tl-icon", KIND_ICON[en.kind] || "•");
    const body = tlEl("div", "tl-body");
    const titleRow = tlEl("div", "tl-title-row");
    titleRow.append(tlEl("span", "tl-kind", KIND_LABEL[en.kind] || en.kind));
    const title = tlEl("span", "tl-title", en.title);
    titleRow.append(title);
    titleRow.append(tlEl("span", "tl-step", "Step " + en.step + " of " + TL.entries.length));
    body.append(titleRow);
    if (en.files.length) body.append(tlEl("div", "tl-files", en.files.join(", ")));
    if (en.detail) body.append(tlEl("div", "tl-detail", en.detail));
    const actions = tlEl("div", "tl-actions");
    if (en.undoable && en.checkpoint_id) {
      const btn = tlEl("button", "tl-undo-btn", "↩ Undo just this");
      btn.title = "Revert this action via checkpoint " + en.checkpoint_id;
      btn.onclick = () => undoEntry(en);
      actions.append(btn);
      actions.append(tlEl("span", "tl-cp", "checkpoint " + en.checkpoint_id));
    }
    if (en.status === "error") actions.append(tlEl("span", "tl-err", "✗ failed"));
    if (en.status === "blocked") actions.append(tlEl("span", "tl-blocked-note", "waiting on you"));
    body.append(actions);
    li.append(icon, body);
    list.append(li);

    void i;
  });
  panel.append(list);

  if (TL.checkpoints.length) {
    panel.append(tlEl("div", "tl-head", "Restore points"));
    const cps = tlEl("ul", "tl-checkpoints");
    TL.checkpoints.forEach(cp => {
      const li = tlEl("li", "tl-cp-row");
      const info = tlEl("div", "tl-cp-info");
      info.append(tlEl("span", "tl-cp-id", cp.id));
      info.append(tlEl("span", "tl-cp-msg", cp.message || ""));
      const btn = tlEl("button", "tl-restore-btn", "⟲ Restore to this point");
      btn.onclick = () => restoreTo(cp.id);
      li.append(info, btn);
      cps.append(li);
    });
    panel.append(cps);
  }

  const note = tlEl("div", "tl-note",
    "Restoring always takes a safety checkpoint first - so a restore can itself be undone.");
  panel.append(note);
}

async function undoEntry(entry) {
  if (!entry.checkpoint_id) return;
  const ok = await confirmNative("Undo action", "Undo \"" + entry.title + "\"?\n\nThis reverts the files via checkpoint " +
    entry.checkpoint_id + ". A safety checkpoint is taken first, so you can undo this undo.");
  if (!ok) return;
  await doRestore(entry.checkpoint_id, "Undone: " + entry.title);
}

async function restoreTo(checkpointId) {
  const ok = await confirmNative("Restore checkpoint", "Restore the workspace to checkpoint " + checkpointId +
    "?\n\nA safety checkpoint is taken first, so you can undo this restore.");
  if (!ok) return;
  await doRestore(checkpointId, "Restored to checkpoint " + checkpointId);
}

async function doRestore(checkpointId, doneMsg) {
  const panel = document.getElementById("timeline-panel");
  const status = tlEl("div", "tl-restore-status", "Restoring…");
  panel.prepend(status);
  try {
    if (!invokeTl) {

      await new Promise(r => setTimeout(r, 600));
      status.textContent = "✓ (demo) " + doneMsg + ". Safety checkpoint demo999 taken - this restore is undoable.";
      status.classList.add("ok");
      return;
    }
    const res = await invokeTl("restore_checkpoint", {
      sessionId: TL.sessionId, checkpointId,
    });
    status.textContent = "✓ " + doneMsg + ". Restored " + res.restored_files.length +
      " file(s). Safety checkpoint " + res.safety_checkpoint_id + " taken - this restore is undoable.";
    status.classList.add("ok");

    await loadTimeline(TL.sessionId);
  } catch (e) {
    status.textContent = "✗ Restore failed: " + String(e);
    status.classList.add("err");
  }
}

function showTimelineTab() {
  document.getElementById("messages").classList.add("hidden");
  document.getElementById("composer").classList.add("hidden");
  document.getElementById("timeline-panel").classList.remove("hidden");
  if (typeof hideNetworkTab === "function") hideNetworkTab();
  if (typeof hideTerminalTab === "function") hideTerminalTab();
  document.getElementById("tab-chat").classList.remove("active");
  document.getElementById("tab-timeline").classList.add("active");
  if (TL.sessionId) loadTimeline(TL.sessionId);
}

function showChatTab() {
  document.getElementById("timeline-panel").classList.add("hidden");
  if (typeof hideNetworkTab === "function") hideNetworkTab();
  if (typeof hideTerminalTab === "function") hideTerminalTab();
  document.getElementById("messages").classList.remove("hidden");
  document.getElementById("composer").classList.remove("hidden");
  document.getElementById("tab-timeline").classList.remove("active");
  document.getElementById("tab-chat").classList.add("active");
}

function timelineSessionChanged(sessionId) {
  TL.sessionId = sessionId;

  if (!document.getElementById("timeline-panel").classList.contains("hidden")) {
    loadTimeline(sessionId);
  }
}
