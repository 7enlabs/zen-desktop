"use strict";

const OnboardingStep = Object.freeze({
  WELCOME: "welcome",
  MODEL_SELECT: "model_select",
  BYOK_SETUP: "byok_setup",
  DEMO_TASK: "demo_task",
  DONE: "done",
  SKIPPED: "skipped",
});

const ONBOARDING_STORAGE_KEY = "zencode.onboarding.v1";

// Endpoint + keyless flags verified 2026-11-05 from the work box (curl, no key):
// - https://ai-gateway.kilo.ai/api/v1/models lists nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free;
//   POST /chat/completions (max_tokens 1, "ping") returns 200, so keyless: true holds.
// - https://api.blazerail.com/v1/models lists deepseek/deepseek-v4.1-flash:free;
//   POST /chat/completions without a key returns 401, so keyless: false holds.
const FREE_MODELS = Object.freeze([
  {
    id: "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
    label: "Nemotron 3 Nano (keyless)",
    baseUrl: "https://ai-gateway.kilo.ai/api/v1",
    keyless: true,
  },
  {
    id: "deepseek/deepseek-v4.1-flash:free",
    label: "DeepSeek V4.1 Flash (BlazeRail)",
    baseUrl: "https://api.blazerail.com/v1",
    keyless: false,
  },
]);

const BAN_RISK = Object.freeze({
  anthropic: {
    level: "HIGH",
    message:
      "Do NOT sign in with a Claude Pro/Max subscription here - Anthropic bans " +
      "third-party apps using subscription OAuth. Use an Anthropic API key instead.",
  },
  google: {
    level: "HIGH",
    message:
      "Do NOT sign in with Google AI Ultra / Antigravity here - Google issues " +
      "mass 403 bans for third-party subscription use and may affect your whole " +
      "Google account. Use an AI Studio API key instead.",
  },
  openai: {
    level: "LOW",
    message:
      "Sign in with ChatGPT (SIWC) is an official OpenAI program and is safe to use.",
  },
});

function banRiskFor(provider) {
  const key = String(provider || "").toLowerCase();
  return BAN_RISK[key] || { level: "UNKNOWN", message: "Unknown provider - proceed with caution." };
}

class OnboardingStateMachine {
  constructor() {
    this.step = OnboardingStep.WELCOME;
    this.selectedModel = null;
    this.byok = null;
    this.demoCompleted = false;
    this.startedAt = Date.now();
  }

  canGoBack() {
    return (
      this.step === OnboardingStep.MODEL_SELECT ||
      this.step === OnboardingStep.BYOK_SETUP ||
      this.step === OnboardingStep.DEMO_TASK
    );
  }

  back() {
    switch (this.step) {
      case OnboardingStep.MODEL_SELECT:
        this.step = OnboardingStep.WELCOME;
        return true;
      case OnboardingStep.BYOK_SETUP:
        this.step = OnboardingStep.MODEL_SELECT;
        return true;
      case OnboardingStep.DEMO_TASK:
        this.step = this.byok ? OnboardingStep.BYOK_SETUP : OnboardingStep.MODEL_SELECT;
        return true;
      default:
        return false;
    }
  }

  begin() {
    if (this.step !== OnboardingStep.WELCOME) return false;
    this.step = OnboardingStep.MODEL_SELECT;
    return true;
  }

  selectModel(model) {
    if (this.step !== OnboardingStep.MODEL_SELECT) return false;
    if (!model || typeof model.id !== "string" || !model.id) return false;
    this.selectedModel = { id: model.id, label: model.label || model.id, baseUrl: model.baseUrl || "" };
    this.step = OnboardingStep.DEMO_TASK;
    return true;
  }

  chooseByok() {
    if (this.step !== OnboardingStep.MODEL_SELECT) return false;
    this.step = OnboardingStep.BYOK_SETUP;
    return true;
  }

  setByok(provider, keyRef) {
    if (this.step !== OnboardingStep.BYOK_SETUP) return false;
    if (!provider || !keyRef) return false;
    this.byok = { provider: String(provider), keyRef: String(keyRef) };
    this.step = OnboardingStep.DEMO_TASK;
    return true;
  }

  completeDemo() {
    if (this.step !== OnboardingStep.DEMO_TASK) return false;
    this.demoCompleted = true;
    this.step = OnboardingStep.DONE;
    return true;
  }

  skip() {
    if (this.step === OnboardingStep.DONE || this.step === OnboardingStep.SKIPPED) return false;
    this.step = OnboardingStep.SKIPPED;
    return true;
  }

  isFinished() {
    return this.step === OnboardingStep.DONE || this.step === OnboardingStep.SKIPPED;
  }

  elapsedSeconds() {
    return Math.floor((Date.now() - this.startedAt) / 1000);
  }

  summary() {
    return {
      step: this.step,
      model: this.selectedModel ? this.selectedModel.id : null,
      byokProvider: this.byok ? this.byok.provider : null,
      demoCompleted: this.demoCompleted,
      elapsedSeconds: this.elapsedSeconds(),
      withinBudget: this.elapsedSeconds() <= 60,
    };
  }
}

function loadOnboardingRecord(storage) {
  try {
    const raw = storage.getItem(ONBOARDING_STORAGE_KEY);
    if (!raw) return null;
    const rec = JSON.parse(raw);
    if (rec && (rec.completed === true || rec.skipped === true)) return rec;
    return null;
  } catch {
    return null;
  }
}

function saveOnboardingRecord(storage, { completed, skipped, modelId }) {
  const rec = {
    completed: !!completed,
    skipped: !!skipped,
    modelId: modelId || null,
    at: new Date().toISOString(),
    v: 1,
  };
  storage.setItem(ONBOARDING_STORAGE_KEY, JSON.stringify(rec));
  return rec;
}

function shouldShowOnboarding(storage) {
  return loadOnboardingRecord(storage) === null;
}

async function probeModel(model, fetchFn, timeoutMs = 8000) {
  const started = Date.now();
  try {
    const ctrl = new AbortController();
    const timer = setTimeout(() => ctrl.abort(), timeoutMs);

    const res = await fetchFn(`${model.baseUrl}/chat/completions`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        model: model.id,
        messages: [{ role: "user", content: "ping" }],
        max_tokens: 1,
      }),
      signal: ctrl.signal,
    });
    clearTimeout(timer);
    return { id: model.id, alive: res && res.ok === true, latencyMs: Date.now() - started };
  } catch {
    return { id: model.id, alive: false, latencyMs: Date.now() - started };
  }
}

async function probeFreeModels(fetchFn, timeoutMs = 8000) {
  const results = await Promise.all(FREE_MODELS.map((m) => probeModel(m, fetchFn, timeoutMs)));
  return results.filter((r) => r.alive).sort((a, b) => a.latencyMs - b.latencyMs);
}

function recommendedDefaultModel(probeResults) {
  if (probeResults.length > 0) {
    const winner = FREE_MODELS.find((m) => m.id === probeResults[0].id);
    if (winner) return winner;
  }
  return FREE_MODELS.find((m) => m.keyless) || FREE_MODELS[0];
}

const DEMO_PROMPT = "Explain in one sentence what ZenCode Desktop is.";

function buildDemoRequest(model) {
  return {
    model: model.id,
    messages: [{ role: "user", content: DEMO_PROMPT }],
    max_tokens: 120,
  };
}

class OnboardingUI {
  constructor({ document, machine, onFinish }) {
    this.document = document;
    this.machine = machine;
    this.onFinish = onFinish;
    this.overlay = null;
  }

  render() {
    const doc = this.document;
    this.overlay = doc.createElement("div");
    this.overlay.id = "onboarding-overlay";
    this.overlay.innerHTML = `
      <div class="onboarding-card" role="dialog" aria-modal="true" aria-label="Welcome to ZenCode">
        <div class="onboarding-progress"><span id="ob-step-label"></span><button id="ob-skip" class="link">Skip</button></div>
        <div id="ob-body"></div>
        <div class="onboarding-nav">
          <button id="ob-back">← Back</button>
          <button id="ob-next" class="primary">Continue →</button>
        </div>
      </div>`;
    doc.body.appendChild(this.overlay);
    this.overlay.querySelector("#ob-skip").addEventListener("click", () => this.finish(false, true));
    this.overlay.querySelector("#ob-back").addEventListener("click", () => {
      if (this.machine.back()) this.renderStep();
    });
    this.overlay.querySelector("#ob-next").addEventListener("click", () => this.handleNext());
    this.renderStep();
  }

  renderStep() {
    const body = this.overlay.querySelector("#ob-body");
    const label = this.overlay.querySelector("#ob-step-label");
    const backBtn = this.overlay.querySelector("#ob-back");
    const nextBtn = this.overlay.querySelector("#ob-next");
    backBtn.style.visibility = this.machine.canGoBack() ? "visible" : "hidden";

    switch (this.machine.step) {
      case OnboardingStep.WELCOME:
        label.textContent = "Step 1 of 3 - Welcome";
        nextBtn.textContent = "Get started →";
        body.innerHTML = `
          <h2>⬢ Welcome to ZenCode Desktop</h2>
          <p>The AI coding agent that <strong>cannot bill you</strong>.</p>
          <p>In the next 60 seconds you'll pick a <strong>free</strong> model and run your first task.</p>`;
        break;
      case OnboardingStep.MODEL_SELECT:
        label.textContent = "Step 2 of 3 - Pick a free model";
        nextBtn.textContent = "Use this model →";
        body.innerHTML = `
          <h2>Pick a free model</h2>
          <p id="ob-probe-status">Probing free models…</p>
          <ul id="ob-model-list"></ul>
          <p><button id="ob-byok-link" class="link">I have my own API key (BYOK)</button></p>`;
        body.querySelector("#ob-byok-link").addEventListener("click", () => {
          this.machine.chooseByok();
          this.renderStep();
        });
        this.renderModelList(body);
        break;
      case OnboardingStep.BYOK_SETUP: {
        label.textContent = "Step 2 of 3 - Bring your own key";
        nextBtn.textContent = "Continue →";
        const risk = banRiskFor(this._byokProvider || "openai");
        body.innerHTML = `
          <h2>Bring your own key <span class="pill">optional</span></h2>
          <label>Provider
            <select id="ob-provider">
              <option value="openai">OpenAI</option>
              <option value="anthropic">Anthropic</option>
              <option value="google">Google AI Studio</option>
            </select>
          </label>
          <div id="ob-banwarn" class="banwarn banwarn-${risk.level.toLowerCase()}">⚠ [${risk.level}] ${risk.message}</div>
          <label>API key reference <input id="ob-keyref" type="text" placeholder="e.g. keychain / vault ref" /></label>
          <p class="hint">Your key is stored in the OS keychain - never in plain text.</p>`;
        const sel = body.querySelector("#ob-provider");
        sel.addEventListener("change", () => {
          this._byokProvider = sel.value;
          const r = banRiskFor(sel.value);
          const w = body.querySelector("#ob-banwarn");
          w.className = `banwarn banwarn-${r.level.toLowerCase()}`;
          w.textContent = `⚠ [${r.level}] ${r.message}`;
        });
        break;
      }
      case OnboardingStep.DEMO_TASK:
        label.textContent = "Step 3 of 3 - First task";
        nextBtn.textContent = "Run demo task ⚡";
        body.innerHTML = `
          <h2>Run your first task</h2>
          <p>Model: <strong>${this.machine.selectedModel ? this.machine.selectedModel.label : "-"}</strong></p>
          <p class="hint">"${DEMO_PROMPT}"</p>
          <div id="ob-demo-out" class="demo-out hidden"></div>`;
        break;
      default:
        break;
    }
  }

  async renderModelList(body) {
    const list = body.querySelector("#ob-model-list");
    const status = body.querySelector("#ob-probe-status");
    const fetchFn = typeof fetch !== "undefined" ? fetch.bind(globalThis) : null;
    let alive = [];
    if (fetchFn) {
      try {
        alive = await probeFreeModels(fetchFn);
      } catch {  }
    }
    const aliveIds = new Set(alive.map((r) => r.id));
    const def = recommendedDefaultModel(alive);
    status.textContent = alive.length > 0
      ? `✓ ${alive.length} free model(s) responding - recommended: ${def.label}`
      : "Could not reach free endpoints - showing defaults (you can still continue).";
    list.innerHTML = FREE_MODELS.map((m) => `
      <li><label>
        <input type="radio" name="ob-model" value="${m.id}" ${m.id === def.id ? "checked" : ""} />
        <strong>${m.label}</strong>
        ${aliveIds.has(m.id) ? '<span class="pill ok">online</span>' : '<span class="pill">offline?</span>'}
        <span class="hint">${m.id}</span>
      </label></li>`).join("");
    this._probedDefault = def;
  }

  handleNext() {
    const m = this.machine;
    const body = this.overlay.querySelector("#ob-body");
    switch (m.step) {
      case OnboardingStep.WELCOME:
        m.begin();
        break;
      case OnboardingStep.MODEL_SELECT: {
        const checked = body.querySelector('input[name="ob-model"]:checked');
        const pick = checked
          ? FREE_MODELS.find((x) => x.id === checked.value)
          : this._probedDefault || FREE_MODELS[0];
        if (!m.selectModel(pick)) return;
        break;
      }
      case OnboardingStep.BYOK_SETUP: {
        const provider = body.querySelector("#ob-provider").value;
        const keyRef = body.querySelector("#ob-keyref").value.trim() || "user-keychain";
        if (!m.setByok(provider, keyRef)) return;
        break;
      }
      case OnboardingStep.DEMO_TASK:

        this.runDemoTask();
        return;
      default:
        return;
    }
    this.renderStep();
  }

  async runDemoTask() {
    const out = this.overlay.querySelector("#ob-demo-out");
    out.classList.remove("hidden");
    out.textContent = "Running…";
    const nextBtn = this.overlay.querySelector("#ob-next");
    nextBtn.disabled = true;
    try {

      const model = this.machine.selectedModel;
      const req = buildDemoRequest(model);
      let text = "";
      if (typeof fetch !== "undefined" && model.baseUrl) {
        const res = await fetch(`${model.baseUrl}/chat/completions`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(req),
        });
        const data = await res.json();
        text = (data.choices && data.choices[0] && data.choices[0].message.content) || "(empty response)";
      } else {
        text = "(demo) ZenCode Desktop is the AI coding agent that cannot bill you.";
      }
      out.textContent = text.trim();
    } catch (e) {
      out.textContent = `Demo task failed (${e.message}). You can still finish setup - the app works offline with local models.`;
    } finally {
      nextBtn.disabled = false;
      nextBtn.textContent = "Finish ✓";
      nextBtn.onclick = () => {
        this.machine.completeDemo();
        this.finish(true, false);
      };
    }
  }

  finish(completed, skipped) {
    if (this.overlay && this.overlay.parentNode) this.overlay.parentNode.removeChild(this.overlay);
    this.onFinish({
      completed,
      skipped,
      modelId: this.machine.selectedModel ? this.machine.selectedModel.id : null,
    });
  }
}

function maybeStartOnboarding({ document, storage, onFinish }) {
  if (!shouldShowOnboarding(storage)) return null;
  const machine = new OnboardingStateMachine();
  const ui = new OnboardingUI({
    document,
    machine,
    onFinish: (result) => {
      saveOnboardingRecord(storage, {
        completed: result.completed,
        skipped: result.skipped,
        modelId: result.modelId,
      });
      if (typeof onFinish === "function") onFinish(result);
    },
  });
  ui.render();
  return ui;
}

if (typeof module !== "undefined" && module.exports) {
  module.exports = {
    OnboardingStep,
    OnboardingStateMachine,
    FREE_MODELS,
    BAN_RISK,
    banRiskFor,
    ONBOARDING_STORAGE_KEY,
    loadOnboardingRecord,
    saveOnboardingRecord,
    shouldShowOnboarding,
    probeModel,
    probeFreeModels,
    recommendedDefaultModel,
    DEMO_PROMPT,
    buildDemoRequest,
    OnboardingUI,
    maybeStartOnboarding,
  };
}
