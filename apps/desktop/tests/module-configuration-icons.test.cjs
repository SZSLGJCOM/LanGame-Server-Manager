const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
const PNG = "data:image/png;base64,iVBORw0KGgo=";
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("module.exports = {};", module.filename);

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((accept, fail) => { resolve = accept; reject = fail; });
  return { promise, resolve, reject };
}

function details(moduleId = "dontstarve") {
  return { summary: { id: moduleId, name: moduleId, install_state: "Installed" } };
}

function hookHarness(shared = { requests: [], loaded: null, activeReact: null }) {
  const hooks = [];
  const effects = [];
  const requests = shared.requests;
  let cursor = 0;
  let writes = 0;
  const equal = (left, right) => left?.length === right?.length && left.every((value, index) => Object.is(value, right[index]));
  const react = {
    useState(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = { value: typeof initial === "function" ? initial() : initial };
      return [hooks[slot].value, (next) => {
        hooks[slot].value = typeof next === "function" ? next(hooks[slot].value) : next;
        writes += 1;
      }];
    },
    useMemo(create, deps) {
      const slot = cursor++;
      if (!equal(hooks[slot]?.deps, deps)) hooks[slot] = { value: create(), deps };
      return hooks[slot].value;
    },
    useRef(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = { current: initial };
      return hooks[slot];
    },
    useEffect(create, deps) {
      const slot = cursor++;
      if (equal(hooks[slot]?.deps, deps)) return;
      const previous = hooks[slot];
      hooks[slot] = { deps, create };
      effects.push(() => {
        previous?.cleanup?.();
        hooks[slot].cleanup = create();
      });
    }
  };
  if (!shared.loaded) {
    const filename = path.join(desktopRoot, "src/views/settings/useModuleConfigurationIcons.ts");
    const loaded = new Module(filename, module);
    loaded.filename = filename;
    const requireFromFile = Module.createRequire(filename);
    loaded.require = (id) => id === "react" ? Object.fromEntries(
      Object.keys(react).map((key) => [key, (...args) => shared.activeReact[key](...args)])
    ) : id === "../../api" ? {
      readModuleConfigurationIcons(moduleId) {
        const request = { moduleId, ...deferred() };
        requests.push(request);
        return request.promise;
      }
    } : requireFromFile(id);
    loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
    shared.loaded = loaded;
  }
  return {
    requests,
    get writes() { return writes; },
    fork() { return hookHarness(shared); },
    render(moduleDetails, instanceId = "server-a", flushEffects = true) {
      cursor = 0;
      shared.activeReact = react;
      const result = shared.loaded.exports.useModuleConfigurationIcons(moduleDetails, instanceId);
      if (flushEffects) this.flush();
      return result;
    },
    flush() { while (effects.length) effects.shift()(); },
    replayEffects() {
      for (const hook of hooks) {
        if (!hook?.create) continue;
        hook.cleanup?.();
        hook.cleanup = hook.create();
      }
    },
    unmount() { for (const hook of hooks) hook?.cleanup?.(); }
  };
}

const settle = () => new Promise((resolve) => setImmediate(resolve));

test("remounting or switching DST instances shares only the same detail object's pending request", async () => {
  const first = hookHarness();
  const moduleDetails = details();
  first.render(moduleDetails, "server-a");
  first.unmount();
  const writesBeforeUnmountedDelivery = first.writes;
  const second = first.fork();
  second.render(moduleDetails, "server-b");
  assert.equal(first.requests.length, 1);
  first.requests[0].resolve({ world_autumn: PNG });
  await settle();
  assert.equal(first.writes, writesBeforeUnmountedDelivery, "the original consumer remains cancelled");
  assert.equal(second.render(moduleDetails, "server-b").icons.world_autumn, PNG);

  const third = first.fork();
  third.render(moduleDetails, "server-c");
  assert.equal(first.requests.length, 2, "completed responses are not retained in the shared request map");
  const differentDetails = first.fork();
  differentDetails.render(details(), "server-d");
  assert.equal(first.requests.length, 3, "new module detail objects never reuse an inferred installation context");
  third.unmount();
  differentDetails.unmount();
  first.requests[1].resolve({});
  first.requests[2].resolve({});
  await settle();
});

test("a shared rejection is removed so each consumer can explicitly retry", async () => {
  const first = hookHarness();
  const second = first.fork();
  const moduleDetails = details();
  first.render(moduleDetails, "server-a");
  second.render(moduleDetails, "server-b");
  assert.equal(first.requests.length, 1);
  first.requests[0].reject(new Error("Decode failed"));
  await settle();
  assert.equal(first.render(moduleDetails, "server-a").error.message, "Decode failed");
  const failed = second.render(moduleDetails, "server-b");
  assert.equal(failed.error.message, "Decode failed");
  failed.retry();
  second.render(moduleDetails, "server-b");
  assert.equal(first.requests.length, 2);
  first.requests[1].resolve({ world_autumn: PNG });
  await settle();
  assert.equal(second.render(moduleDetails, "server-b").error, null);
  assert.equal(first.render(moduleDetails, "server-a").error.message, "Decode failed",
    "another consumer's retry does not reset this hook's failure or retry scope");
});

test("an empty DST icon result stays visible until an explicit reload finds icons", async () => {
  const harness = hookHarness();
  const moduleDetails = details();
  assert.equal(harness.render(details("minecraft")).missing, false);
  assert.equal(harness.requests.length, 0);
  harness.render(moduleDetails);
  harness.render(moduleDetails);
  harness.replayEffects();
  assert.equal(harness.requests.length, 1);
  harness.requests[0].resolve({});
  await settle();
  const result = harness.render(moduleDetails);
  assert.deepEqual(result.icons, {});
  assert.equal(result.error, null);
  assert.equal(result.loading, false);
  assert.equal(result.missing, true);
  assert.equal(result.retryAvailable, true);
  result.retry();
  const pending = harness.render(moduleDetails);
  assert.equal(harness.requests.length, 2);
  assert.equal(pending.missing, true, "keep the explanation visible during the explicit reload");
  assert.equal(pending.loading, true);
  pending.retry();
  harness.render(moduleDetails);
  assert.equal(harness.requests.length, 2, "a pending reload cannot be duplicated");
  harness.requests[1].resolve({ world_autumn: PNG });
  await settle();
  const loaded = harness.render(moduleDetails);
  assert.equal(loaded.icons.world_autumn, PNG);
  assert.equal(loaded.missing, false);
  assert.equal(loaded.error, null);
  assert.equal(loaded.loading, false);
  loaded.retry();
  harness.render(moduleDetails);
  assert.equal(harness.requests.length, 2, "a successful nonempty response needs no reload");
});

test("missing icons allow only two explicit reloads and reset immediately when the scope changes", async () => {
  const harness = hookHarness();
  const moduleDetails = details();
  harness.render(moduleDetails);
  for (let attempt = 0; attempt < 3; attempt += 1) {
    harness.requests[attempt].resolve({});
    await settle();
    const result = harness.render(moduleDetails);
    assert.equal(result.missing, true);
    assert.equal(result.error, null);
    assert.equal(result.retryAvailable, attempt < 2);
    assert.equal(harness.requests.length, attempt + 1, "empty results never trigger automatic reloads");
    result.retry();
    harness.render(moduleDetails);
  }
  assert.equal(harness.requests.length, 3, "an empty result cannot bypass the retry limit");
  const nextInstance = harness.render(moduleDetails, "server-b", false);
  assert.equal(nextInstance.missing, false, "do not show a previous instance's missing-resource notice");
  assert.equal(nextInstance.retryAvailable, true);
  harness.flush();
  const other = details("minecraft");
  assert.equal(harness.render(other, "server-c").missing, false);
  const writesBeforeLateResponse = harness.writes;
  harness.requests[3].resolve({});
  await settle();
  assert.equal(harness.writes, writesBeforeLateResponse);
  assert.equal(harness.render(other, "server-c").missing, false);
});

test("the workspace exposes missing resources as a localized, nonblocking status with bounded reload", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src/views/settings/ConfigurationWorkspace.tsx"), "utf8");
  const condition = source.match(/const showIconNotice = ([\s\S]*?);/);
  assert.ok(condition, "the icon notice must have a section-aware display condition");
  const showNotice = new Function("moduleId", "selectedSectionId", "configurationIcons", `return (${condition[1]});`);
  const worldSections = ["mastergen", "mastersettings", "cavesgen", "cavessettings"];
  for (const section of [...worldSections, "network", "runtime", "identity", null]) {
    const expected = worldSections.includes(section);
    assert.equal(showNotice("dontstarve", section, { missing: true, error: null }), expected, `${section}: missing icons`);
    assert.equal(showNotice("dontstarve", section, { missing: false, error: new Error("Read failed") }), expected, `${section}: failed read`);
    assert.equal(showNotice("dontstarve", section, { missing: false, error: null }), false, `${section}: successful read`);
    assert.equal(showNotice("minecraft", section, { missing: true, error: null }), false, `${section}: unrelated module`);
  }
  const start = source.indexOf("{showIconNotice ?");
  const end = source.indexOf("{saveFailureMessage ?", start);
  assert.ok(start >= 0 && end > start, "both failed and empty icon reads must reach the notice");
  const notice = source.slice(start, end);
  assert.match(notice, /<ActivityNotice\s+tone="warning"/);
  const { ActivityNotice } = require("../src/components/ActivityNotice.tsx");
  const renderedNotice = renderToStaticMarkup(React.createElement(ActivityNotice,
    { tone: "warning" }, "Icons unavailable"));
  assert.match(renderedNotice, /role="status"/);
  assert.match(notice, /configurationIcons\.missing\s*\? t\("settings\.configuration\.icons\.missing"/);
  assert.match(notice, /configurationIcons\.retryAvailable\s*\?\s*<button/);
  assert.match(notice, /disabled=\{configurationIcons\.loading\} onClick=\{configurationIcons\.retry\}/);
  const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
  const { ZH_CN_SETTINGS_MESSAGES } = require("../src/i18n-messages-zh-settings.ts");
  assert.equal(EN_US_MESSAGES["settings.configuration.icons.missing"],
    "No local game icons were found. Install or update the Don't Starve Together server, then reload the icons.");
  assert.equal(ZH_CN_SETTINGS_MESSAGES["settings.configuration.icons.missing"],
    "未找到本地游戏图标。安装或更新饥荒专服后可重新读取。");
  assert.equal(EN_US_MESSAGES["settings.configuration.icons.retry"], "Reload icons");
  assert.equal(ZH_CN_SETTINGS_MESSAGES["settings.configuration.icons.retry"], "重新读取");
});

test("switching module or instance clears icons before effects and ignores late responses", async () => {
  const harness = hookHarness();
  const first = details();
  harness.render(first);
  harness.requests[0].resolve({ world_autumn: PNG });
  await settle();
  assert.equal(harness.render(first).icons.world_autumn, PNG);
  const next = details();
  assert.deepEqual(harness.render(next, "server-b", false).icons, {});
  harness.flush();
  const other = details("minecraft");
  assert.deepEqual(harness.render(other, "server-c", false).icons, {});
  harness.flush();
  const writesBeforeLateResponse = harness.writes;
  harness.requests[1].resolve({ world_winter: PNG });
  await settle();
  assert.equal(harness.writes, writesBeforeLateResponse);
  assert.deepEqual(harness.render(other, "server-c").icons, {});
  assert.equal(harness.requests.length, 2);
});

test("failed icon loading is nonblocking and allows only two explicit retries", async () => {
  const harness = hookHarness();
  const moduleDetails = details();
  harness.render(moduleDetails);
  for (let attempt = 0; attempt < 3; attempt += 1) {
    harness.requests[attempt].reject(new Error("Atlas could not be read"));
    await settle();
    const result = harness.render(moduleDetails);
    assert.deepEqual(result.icons, {});
    assert.equal(result.error.message, "Atlas could not be read");
    assert.equal(result.missing, false);
    assert.equal(result.loading, false);
    assert.equal(result.retryAvailable, attempt < 2);
    assert.equal(harness.requests.length, attempt + 1, "no automatic retries");
    result.retry();
    const pending = harness.render(moduleDetails);
    if (attempt < 2) {
      assert.equal(pending.loading, true);
      pending.retry();
      harness.render(moduleDetails);
      assert.equal(harness.requests.length, attempt + 2, "pending retries cannot be duplicated");
    }
  }
  assert.equal(harness.requests.length, 3);
  const refreshed = details();
  assert.equal(harness.render(refreshed).error, null, "a new detail version resets the failure scope");
  assert.equal(harness.requests.length, 4);
});

test("invalid image responses become a recoverable error and unmount cancels delivery", async () => {
  const harness = hookHarness();
  const moduleDetails = details();
  harness.render(moduleDetails);
  harness.requests[0].resolve({ world_autumn: "https://example.invalid/image.png" });
  await settle();
  const failed = harness.render(moduleDetails);
  assert.match(failed.error.message, /PNG data URLs/);
  assert.deepEqual(failed.icons, {});
  failed.retry();
  harness.render(moduleDetails);
  harness.unmount();
  const writes = harness.writes;
  harness.requests[1].resolve({ world_autumn: PNG });
  await settle();
  assert.equal(harness.writes, writes);
});

test("runtime icons decorate only DST world fields without changing labels, settings or reachability", () => {
  const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
  const { buildConfigurationWorkspaceModel } = require("../src/views/settings/configuration-workspace-model.ts");
  const schemaJson = fs.readFileSync(path.resolve(desktopRoot, "../../modules/dontstarve/schema.json"), "utf8");
  const moduleDetails = { ...details(), schema_json: schemaJson };
  const original = parseGuidedSettingsSchema(moduleDetails);
  const fieldIcons = Object.fromEntries(original.fields.map((field) => [field.key, PNG]));
  const decorated = parseGuidedSettingsSchema(moduleDetails, "en-US", undefined, { fieldIcons });
  const worldSections = new Set(["mastergen", "mastersettings", "cavesgen", "cavessettings"]);
  assert.equal(decorated.fields.filter((field) => field.icon).length, 280);
  for (const field of decorated.fields) {
    const baseline = original.fields.find((candidate) => candidate.key === field.key);
    assert.equal(field.icon, worldSections.has(field.sectionId) ? PNG : null, field.key);
    assert.deepEqual({ ...field, icon: baseline.icon }, baseline, field.key);
  }
  assert.deepEqual(buildConfigurationWorkspaceModel(decorated).actionableSectionIds,
    buildConfigurationWorkspaceModel(original).actionableSectionIds);
  assert.equal(moduleDetails.schema_json, schemaJson);

  const other = parseGuidedSettingsSchema({ ...moduleDetails, summary: { id: "minecraft", name: "Minecraft" } },
    "en-US", undefined, { fieldIcons });
  assert.ok(other.fields.every((field) => field.icon === null));
});
