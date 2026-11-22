"use strict";

const { test, describe } = require("node:test");
const assert = require("node:assert/strict");

const {
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
} = require("./onboarding.js");

function fakeStorage() {
  const map = new Map();
  return {
    getItem: (k) => (map.has(k) ? map.get(k) : null),
    setItem: (k, v) => map.set(k, String(v)),
  };
}

describe("OnboardingStateMachine - happy path", () => {
  test("WELCOME -> MODEL_SELECT -> DEMO_TASK -> DONE", () => {
    const m = new OnboardingStateMachine();
    assert.equal(m.step, OnboardingStep.WELCOME);
    assert.ok(m.begin());
    assert.equal(m.step, OnboardingStep.MODEL_SELECT);
    assert.ok(m.selectModel(FREE_MODELS[0]));
    assert.equal(m.step, OnboardingStep.DEMO_TASK);
    assert.ok(m.completeDemo());
    assert.equal(m.step, OnboardingStep.DONE);
    assert.ok(m.isFinished());
  });

  test("summary reports model + budget", () => {
    const m = new OnboardingStateMachine();
    m.begin();
    m.selectModel(FREE_MODELS[0]);
    const s = m.summary();
    assert.equal(s.model, FREE_MODELS[0].id);
    assert.equal(s.demoCompleted, false);
    assert.ok(s.elapsedSeconds >= 0);
    assert.equal(typeof s.withinBudget, "boolean");
  });
});

describe("OnboardingStateMachine - BYOK detour", () => {
  test("MODEL_SELECT -> BYOK_SETUP -> DEMO_TASK with ban-risk lookup", () => {
    const m = new OnboardingStateMachine();
    m.begin();
    assert.ok(m.chooseByok());
    assert.equal(m.step, OnboardingStep.BYOK_SETUP);

    assert.equal(banRiskFor("https://api.anthropic.com").level, "HIGH");
    assert.equal(banRiskFor("https://generativelanguage.googleapis.com/v1beta/openai/").level, "HIGH");
    assert.equal(banRiskFor("https://api.openai.com/v1").level, "LOW");
    assert.ok(m.setByok("deepseek-v4.1-flash", "https://api.experientiallabs.ai/api/v1", "keychain-ref-1"));
    assert.equal(m.step, OnboardingStep.DEMO_TASK);
    assert.equal(m.summary().byokModel, "deepseek-v4.1-flash");
    assert.equal(m.summary().byokBaseUrl, "https://api.experientiallabs.ai/api/v1");
  });

  test("setByok rejects empty model/baseUrl/keyRef", () => {
    const m = new OnboardingStateMachine();
    m.begin();
    m.chooseByok();
    assert.equal(m.setByok("", "https://api.experientiallabs.ai/api/v1", "ref"), false);
    assert.equal(m.setByok("deepseek-v4.1-flash", "", "ref"), false);
    assert.equal(m.setByok("deepseek-v4.1-flash", "https://api.experientiallabs.ai/api/v1", ""), false);
    assert.equal(m.step, OnboardingStep.BYOK_SETUP);
  });

  test("back() from BYOK_SETUP returns to MODEL_SELECT", () => {
    const m = new OnboardingStateMachine();
    m.begin();
    m.chooseByok();
    assert.ok(m.back());
    assert.equal(m.step, OnboardingStep.MODEL_SELECT);
  });
});

describe("OnboardingStateMachine - skip & invalid transitions", () => {
  test("skip works from any unfinished step", () => {
    for (const setup of [
      (m) => {},
      (m) => m.begin(),
      (m) => { m.begin(); m.chooseByok(); },
      (m) => { m.begin(); m.selectModel(FREE_MODELS[0]); },
    ]) {
      const m = new OnboardingStateMachine();
      setup(m);
      assert.ok(m.skip(), "skip should succeed");
      assert.equal(m.step, OnboardingStep.SKIPPED);
      assert.ok(m.isFinished());
    }
  });

  test("skip after DONE is rejected", () => {
    const m = new OnboardingStateMachine();
    m.begin();
    m.selectModel(FREE_MODELS[0]);
    m.completeDemo();
    assert.equal(m.skip(), false);
  });

  test("out-of-order transitions are rejected", () => {
    const m = new OnboardingStateMachine();
    assert.equal(m.selectModel(FREE_MODELS[0]), false);
    assert.equal(m.completeDemo(), false);
    assert.equal(m.setByok("deepseek-v4.1-flash", "https://api.experientiallabs.ai/api/v1", "r"), false);
    assert.equal(m.step, OnboardingStep.WELCOME);
  });

  test("selectModel rejects empty model", () => {
    const m = new OnboardingStateMachine();
    m.begin();
    assert.equal(m.selectModel(null), false);
    assert.equal(m.selectModel({}), false);
    assert.equal(m.step, OnboardingStep.MODEL_SELECT);
  });

  test("back() from WELCOME fails", () => {
    const m = new OnboardingStateMachine();
    assert.equal(m.back(), false);
  });
});

describe("persistence", () => {
  test("shouldShowOnboarding true on first run, false after completion", () => {
    const s = fakeStorage();
    assert.ok(shouldShowOnboarding(s));
    saveOnboardingRecord(s, { completed: true, skipped: false, modelId: FREE_MODELS[0].id });
    assert.equal(shouldShowOnboarding(s), false);
    const rec = loadOnboardingRecord(s);
    assert.equal(rec.modelId, FREE_MODELS[0].id);
    assert.equal(rec.completed, true);
  });

  test("skipped record also suppresses onboarding", () => {
    const s = fakeStorage();
    saveOnboardingRecord(s, { completed: false, skipped: true, modelId: null });
    assert.equal(shouldShowOnboarding(s), false);
  });

  test("corrupt JSON is treated as no record", () => {
    const s = fakeStorage();
    s.setItem(ONBOARDING_STORAGE_KEY, "not-json{{{");
    assert.equal(loadOnboardingRecord(s), null);
    assert.ok(shouldShowOnboarding(s));
  });
});

describe("probe + recommendation", () => {
  test("probeModel reports alive on ok response", async () => {
    const fakeFetch = async () => ({ ok: true });
    const r = await probeModel(FREE_MODELS[0], fakeFetch, 1000);
    assert.equal(r.alive, true);
    assert.equal(r.id, FREE_MODELS[0].id);
  });

  test("probeModel reports dead on throw", async () => {
    const fakeFetch = async () => { throw new Error("boom"); };
    const r = await probeModel(FREE_MODELS[0], fakeFetch, 1000);
    assert.equal(r.alive, false);
  });

  test("probeFreeModels reports the default model alive", async () => {
    const fakeFetch = async () => ({ ok: true });
    const alive = await probeFreeModels(fakeFetch, 2000);
    assert.equal(alive.length, FREE_MODELS.length);
    assert.equal(alive[0].id, FREE_MODELS[0].id);
  });

  test("recommendedDefaultModel prefers fastest alive, else the default model", () => {
    const rec = recommendedDefaultModel([{ id: FREE_MODELS[0].id, alive: true, latencyMs: 10 }]);
    assert.equal(rec.id, FREE_MODELS[0].id);
    const fallback = recommendedDefaultModel([]);
    assert.equal(fallback.id, FREE_MODELS[0].id);
  });
});

describe("demo task", () => {
  test("buildDemoRequest shapes a valid chat request", () => {
    const req = buildDemoRequest(FREE_MODELS[0]);
    assert.equal(req.model, FREE_MODELS[0].id);
    assert.ok(Array.isArray(req.messages) && req.messages.length === 1);
    assert.ok(DEMO_PROMPT.length > 0);
  });

  test("BAN_RISK is keyed by base URL", () => {
    assert.ok(BAN_RISK["https://api.anthropic.com"]);
    assert.ok(BAN_RISK["https://generativelanguage.googleapis.com/v1beta/openai"]);
    assert.ok(BAN_RISK["https://api.openai.com/v1"]);
    assert.equal(banRiskFor("HTTPS://API.ANTHROPIC.COM/").level, "HIGH");
    assert.equal(banRiskFor("unknown-xyz").level, "UNKNOWN");
  });
});
