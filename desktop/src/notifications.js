export const isTauri = typeof window !== "undefined" && !!window.__TAURI__;

let invokeFn = null;
if (isTauri) {

  import("@tauri-apps/api/core")
    .then((m) => {
      invokeFn = m.invoke;
    })
    .catch(() => {
      invokeFn = null;
    });
}

function ensureToastContainer() {
  let el = document.getElementById("zc-toast-container");
  if (!el) {
    el = document.createElement("div");
    el.id = "zc-toast-container";
    el.style.cssText =
      "position:fixed;top:16px;right:16px;z-index:9999;display:flex;" +
      "flex-direction:column;gap:8px;max-width:360px;";
    document.body.appendChild(el);
  }
  return el;
}

export function showToast(title, body, kind = "info") {
  const container = ensureToastContainer();
  const toast = document.createElement("div");
  const accent =
    kind === "approval"
      ? "#f0883e"
      : kind === "failed"
        ? "#f85149"
        : "#3fb950";
  toast.style.cssText =
    "background:#161b22;border:1px solid #30363d;border-left:3px solid " +
    accent +
    ";border-radius:8px;padding:10px 14px;color:#e6edf3;" +
    "font-family:system-ui,sans-serif;font-size:13px;" +
    "box-shadow:0 8px 24px rgba(0,0,0,.5);cursor:pointer;";
  toast.innerHTML =
    `<div style="font-weight:600;margin-bottom:2px;">${escapeHtml(title)}</div>` +
    `<div style="color:#8b949e;">${escapeHtml(body)}</div>`;
  toast.addEventListener("click", () => toast.remove());
  container.appendChild(toast);

  if (kind !== "approval") {
    setTimeout(() => toast.remove(), 8000);
  }
  return toast;
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => {
    const map = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
    return map[c];
  });
}

async function invokeOrToast(command, args, toastTitle, toastBody, toastKind) {
  if (invokeFn) {
    try {
      await invokeFn(command, args);
      return;
    } catch (e) {
      console.warn(`[notifications] ${command} failed, falling back to toast:`, e);
    }
  }
  showToast(toastTitle, toastBody, toastKind);
}

export function notifySessionEnd(envelope) {
  const payload = envelope.payload || {};
  const sessionId = envelope.session_id || payload.session_id || "unknown";
  const summary = payload.summary || payload.result || "task completed";
  const success = payload.success !== false;
  return invokeOrToast(
    "cmd_notify_session_end",
    { sessionId, summary, success },
    success ? "Zen - task finished" : "Zen - task failed",
    `${summary} (${sessionId})`,
    success ? "info" : "failed",
  );
}

export function notifyApprovalNeeded(toolName, sessionId) {
  return invokeOrToast(
    "cmd_notify_approval_needed",
    { toolName, sessionId },
    "Zen - approval needed",
    `Agent is waiting for approval: ${toolName}`,
    "approval",
  );
}

export function routeEvent(envelope, opts = {}) {
  const type = envelope.type || "";
  const payload = envelope.payload || {};

  if (type === "session_end" || type === "SessionEnd") {
    notifySessionEnd(envelope);
    return true;
  }
  if (type === "permission_denied" || type === "PermissionDenied") {
    const tool = payload.tool_name || payload.tool || "unknown tool";
    notifyApprovalNeeded(tool, envelope.session_id || "unknown");
    return true;
  }
  if (
    (type === "tool_result" || type === "ToolResult") &&
    payload.is_error === true &&
    opts.runEnded === true
  ) {
    notifySessionEnd({ ...envelope, payload: { ...payload, success: false } });
    return true;
  }

  return false;
}

export function demoNotifications() {
  const sessionId = "demo-" + Math.floor(Math.random() * 1000);
  showToast("Zen - task finished", `Refactored auth.ts (12 files changed) (${sessionId})`, "info");
  setTimeout(() => {
    showToast(
      "Zen - approval needed",
      "Agent is waiting for approval: Bash (rm -rf)",
      "approval",
    );
  }, 1500);
}
