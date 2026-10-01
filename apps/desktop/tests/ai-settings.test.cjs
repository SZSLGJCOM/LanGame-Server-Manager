const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const sourcePath = path.join(__dirname, "..", "src", "ai-settings.ts");
const moduleRecord = { exports: {} };
const saved = new Map();
vm.runInNewContext(transpileTypeScript(fs.readFileSync(sourcePath, "utf8"), sourcePath), {
  module: moduleRecord,
  exports: moduleRecord.exports,
  require(name) {
    assert.equal(name, "./i18n");
    return { selectLocaleText: (locale, chinese, english) => locale === "zh-CN" ? chinese : english };
  },
  window: { localStorage: { getItem: (key) => saved.get(key), setItem: (key, value) => saved.set(key, value) } }
}, { filename: sourcePath });
const settings = moduleRecord.exports;
const plain = (value) => JSON.parse(JSON.stringify(value));

test("provider choices expose only supported protocols and require an explicit model", () => {
  assert.deepEqual(plain(settings.listAiProviderPresets().map((preset) => preset.id)), [
    "openai-compatible", "anthropic-compatible", "ollama"
  ]);
  for (const preset of settings.listAiProviderPresets()) {
    const defaults = settings.createDefaultAiSettings(preset.id);
    assert.equal(defaults.model, "");
    assert.equal(settings.getAiSettingsStatus(defaults).ready, false);
    assert.ok(settings.getAiSettingsStatus(defaults).missing.includes("model"));
  }
});

test("removed and invalid providers cannot carry credentials into a default protocol", () => {
  for (const provider of ["openai", "deepseek", "anthropic", "unknown", undefined]) {
    const normalized = settings.normalizeAiSettings({
      provider, enabled: true, model: "previous-model", baseUrl: "https://previous.example/v1",
      apiKey: "fixture-client-secret", apiKeyStored: true
    });
    assert.deepEqual(plain(normalized), plain(settings.createDefaultAiSettings()));
    assert.equal(settings.getAiSettingsStatus(normalized).ready, false);
  }
});

test("switching protocol clears the previous endpoint, model, and credentials", () => {
  const current = { ...settings.createDefaultAiSettings(), model: "previous-model", apiKey: "fixture-client-secret", apiKeyStored: true };
  const changed = settings.applyAiProviderPreset(current, "anthropic-compatible");
  assert.equal(changed.baseUrl, "https://api.anthropic.com/v1");
  assert.equal(changed.model, "");
  assert.equal(changed.apiKey, "");
  assert.equal(changed.apiKeyStored, false);
});

test("changing service URL cannot reuse the previous gateway credential", () => {
  const current = { ...settings.createDefaultAiSettings(), apiKey: "fixture-client-secret", apiKeyStored: true };
  const changed = settings.applyAiServiceUrl(current, "https://another.example/v1");
  assert.equal(changed.apiKey, "");
  assert.equal(changed.apiKeyStored, false);
  assert.strictEqual(settings.applyAiServiceUrl(current, current.baseUrl), current);
});

test("settings writes and key clears are serialized and only the newest state is published", async () => {
  const queue = new settings.AiSettingsWriteQueue();
  let finishWrite;
  let credential = "";
  const initialRevision = queue.revision;
  const first = queue.run(async () => {
    await new Promise((resolve) => { finishWrite = resolve; });
    credential = "written-key";
    return { ...settings.createDefaultAiSettings(), apiKeyStored: true };
  });
  const second = queue.run(async () => {
    assert.equal(credential, "written-key");
    credential = "";
    return settings.createDefaultAiSettings("anthropic-compatible");
  });
  assert.notEqual(queue.revision, initialRevision);
  await Promise.resolve();
  finishWrite();
  assert.equal(await first, null);
  assert.equal((await second).provider, "anthropic-compatible");
  assert.equal(credential, "");
});

test("a failed settings write is reported without preventing later writes", async () => {
  const queue = new settings.AiSettingsWriteQueue();
  const failed = queue.run(async () => { throw new Error("fixture failure"); });
  const recovered = queue.run(async () => settings.createDefaultAiSettings("ollama"));
  await assert.rejects(failed, /fixture failure/);
  assert.equal((await recovered).provider, "ollama");
});

test("settings persistence rejects excess work while the credential store is blocked", async () => {
  const queue = new settings.AiSettingsWriteQueue();
  let finishWrite;
  const pending = [queue.run(async () => {
    await new Promise((resolve) => { finishWrite = resolve; });
    return settings.createDefaultAiSettings();
  })];
  for (let index = 0; index < 7; index += 1) {
    pending.push(queue.run(async () => settings.createDefaultAiSettings()));
  }
  await assert.rejects(queue.run(async () => settings.createDefaultAiSettings()), /busy/);
  finishWrite();
  await Promise.all(pending);
  assert.notEqual(await queue.run(async () => settings.createDefaultAiSettings()), null);
});

test("valid custom protocol settings keep the chosen model while persistence excludes the key", () => {
  const input = { ...settings.createDefaultAiSettings("anthropic-compatible"), model: " selected-model ", baseUrl: "https://gateway.example/Tenant/v1", apiKey: "typed-key", apiKeyStored: true };
  settings.persistAiSettings(input);
  const reloaded = settings.loadAiSettings();
  assert.equal(reloaded.provider, "anthropic-compatible");
  assert.equal(reloaded.model, "selected-model");
  assert.equal(reloaded.baseUrl, input.baseUrl);
  assert.equal(reloaded.apiKey, "");
  assert.equal(reloaded.apiKeyStored, true);
  assert.equal(settings.getAiSettingsStatus(reloaded).ready, true);
});

test("a delayed AI settings save preserves a newer draft while acknowledging its stored key", () => {
  const submitted = { ...settings.createDefaultAiSettings(), model: "model-a", apiKey: "fixture-key" };
  const persisted = { ...submitted, apiKey: "", apiKeyStored: true };
  const draft = { ...submitted, model: "model-b" };
  const merged = settings.mergePersistedAiSettings(draft, submitted, persisted);
  assert.equal(merged.model, "model-b");
  assert.equal(merged.apiKey, "");
  assert.equal(merged.apiKeyStored, true);
  assert.strictEqual(settings.mergePersistedAiSettings(submitted, submitted, persisted), persisted);
});

test("AI save acknowledgements cannot overwrite a newer key or bind credentials to another endpoint", () => {
  const submitted = { ...settings.createDefaultAiSettings(), model: "model-a", apiKey: "fixture-key" };
  const persisted = { ...submitted, apiKey: "", apiKeyStored: true };
  for (const draft of [
    { ...submitted, apiKey: ["replacement", "fixture", "key"].join("-") },
    settings.applyAiServiceUrl(submitted, "https://second.example/v1"),
    settings.applyAiProviderPreset(submitted, "ollama")
  ]) {
    assert.strictEqual(settings.mergePersistedAiSettings(draft, submitted, persisted), draft);
  }
});

test("a delayed credential clear preserves subsequent model and credential edits", () => {
  const submitted = { ...settings.createDefaultAiSettings(), model: "model-a", apiKeyStored: true };
  const cleared = { ...submitted, apiKeyStored: false };
  const changedModel = settings.mergePersistedAiSettings({ ...submitted, model: "model-b" }, submitted, cleared);
  assert.equal(changedModel.model, "model-b");
  assert.equal(changedModel.apiKeyStored, false);
  const changedKey = { ...submitted, apiKey: ["replacement", "fixture", "key"].join("-") };
  assert.strictEqual(settings.mergePersistedAiSettings(changedKey, submitted, cleared), changedKey);
});
