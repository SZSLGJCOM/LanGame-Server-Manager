const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function find(tree, predicate) {
  if (!tree || typeof tree !== "object") return null;
  if (predicate(tree)) return tree;
  for (const child of [tree.props?.children].flat(Infinity)) {
    const result = find(child, predicate);
    if (result) return result;
  }
  return null;
}

function harness() {
  const states = [];
  const refs = [];
  const frames = [];
  let stateIndex = 0;
  let refIndex = 0;
  let phase = "loading";
  let preloads = 0;
  let retries = 0;
  const header = { left: 0, top: 0, width: 1440, height: 86, bottom: 86 };
  const browser = { innerWidth: 1440, requestAnimationFrame: (callback) => frames.push(callback), cancelAnimationFrame() {} };
  const node = (type, props) => typeof type === "function" ? type(props) : { type, props };
  const dependencies = {
    "react/jsx-runtime": { jsx: node, jsxs: node, Fragment: "fragment" },
    react: {
      useCallback: (callback) => callback,
      useEffect: () => {},
      useLayoutEffect: () => {},
      useRef: (initial) => refs[refIndex++] ??= { current: initial },
      useState: (initial) => {
        const index = stateIndex++;
        if (!(index in states)) states[index] = typeof initial === "function" ? initial() : initial;
        return [states[index], (next) => { states[index] = typeof next === "function" ? next(states[index]) : next; }];
      }
    },
    "react-dom": { createPortal: (tree) => tree },
    "../i18n": { useI18n: () => ({ t: (key, _values, fallback) => fallback ?? key }) },
    "../deferred-module": { createDeferredModule: () => ({
      load: () => { preloads++; return Promise.resolve(); }, peek: () => null
    }) },
    "../hooks/useDeferredModule": { useDeferredModule: (_source, enabled) => ({
      status: enabled ? phase : "idle", value: phase === "ready" ? panel.AssistantPanel : null,
      canRetry: phase === "error" && retries === 0,
      retry: () => { retries++; phase = "loading"; }
    }) },
    "../ai-settings": { getAiSettingsStatus: () => ({ ready: true }) },
    "../assistant-state": { buildAssistantViewModel: () => ({
      panelTitle: "LAN", closeLabel: "Close LAN", issues: [], prompts: [], contextPayload: ""
    }) },
    "../hooks/useAssistantPromptRotation": { useAssistantPromptRotation: () => null },
    "./AppAiSettingsCard": { AppAiSettingsCard: "settings" },
    "./PrivacyNotice": { PrivacyNotice: "privacy-content" },
    "./KnowledgeSettingsCard": { KnowledgeSettingsCard: "knowledge-settings" },
    "./AssistantHistoryDrawer": { AssistantHistoryDrawer: "history" },
    "./AssistantUpdateNotice": { AssistantUpdateNotice: "update" },
    "./LanMark": { LanMark: "svg" },
    "./ShellIcon": { ShellIcon: "svg" },
    "./AssistantIslandSurface": { AssistantIslandSurface: (props) => ({ type: "island-surface", props }) },
    "./assistant-focus": { containAssistantFocus() {}, focusAssistantPanel() {} }
  };
  function load(filename) {
    const sourcePath = path.join(__dirname, "../src/components", filename);
    const exports = {};
    vm.runInNewContext(transpileTypeScript(fs.readFileSync(sourcePath, "utf8"), sourcePath), {
      exports, window: browser, document: { body: {} },
      require(id) { assert.ok(Object.hasOwn(dependencies, id), `Unexpected dependency: ${id}`); return dependencies[id]; }
    }, { filename: sourcePath });
    return exports;
  }
  dependencies["./assistant-panel-position"] = load("assistant-panel-position.ts");
  const panel = load("AssistantPanel.tsx");
  const { AssistantCapsule } = load("AssistantCapsule.tsx");
  const props = {
    aiSettings: {}, appUpdateState: { status: "idle" }, assistant: { panelTitle: "LAN", tone: "info" },
    assistantInput: { activeView: "servers", selectedInstanceDetails: { summary: { name: "Local server" } }, storageReady: true },
    assistantDraft: "", execution: { status: "idle" }, messages: [], conversations: [], activeConversationId: null
  };
  function render() {
    stateIndex = refIndex = 0;
    return AssistantCapsule(props);
  }
  const entry = find(render(), (element) => element.type === "button");
  entry.props.ref.current = {
    closest: () => ({ getBoundingClientRect: () => header }),
    getBoundingClientRect: () => ({ left: 657, top: 6, width: 126, height: 37 }), focus() {}
  };
  return {
    render, entry, header, frames,
    phase(value) { phase = value; },
    counts: () => ({ preloads, retries }),
    dialog: () => {
      const surface = find(render(), (element) => element.type === "island-surface");
      return surface?.props.open ? find(surface, (element) => element.props?.className?.includes("assistant-panel--chat")) : null;
    }
  };
}

test("the first LAN click immediately opens the anchored, named and closable loading dialog", () => {
  const ui = harness();
  assert.equal(ui.dialog(), null);
  ui.entry.props.onClick();
  const dialog = ui.dialog();
  assert.ok(dialog, "Opening may not wait for the module or a Suspense reveal delay");
  assert.equal(find(ui.render(), (element) => element.type === "island-surface").props.panelTitle, "LAN");
  assert.equal(dialog.props["aria-label"], "LAN");
  assert.match(dialog.props.className, /is-anchored/);
  assert.equal(dialog.props.style["--assistant-panel-top"], "6px");
  assert.equal(dialog.props.style["--assistant-panel-left"], "480px");
  assert.equal(dialog.props.style["--assistant-panel-width"], "480px");
  assert.equal(find(dialog, (element) => element.props?.className === "assistant-panel-title"), null);
  assert.equal(find(dialog, (element) => element.props?.className === "assistant-panel-context"), null);
  assert.ok(find(dialog, (element) => element.props?.role === "status"));
  const close = find(dialog, (element) => element.props?.["aria-label"] === "Close LAN");
  close.props.onClick();
  assert.equal(ui.dialog(), null);
  assert.equal(ui.frames.length, 1, "Closing must return focus to the LAN entry");
});

test("module failure remains in the LAN dialog, retry shows loading, and success keeps its geometry", () => {
  const ui = harness();
  ui.entry.props.onClick();
  const firstStyle = ui.dialog().props.style;
  ui.phase("error");
  const failed = ui.dialog();
  assert.ok(find(failed, (element) => element.props?.role === "alert"));
  assert.deepEqual(failed.props.style, firstStyle);
  const retry = find(failed, (element) => element.props?.className === "assistant-load-retry");
  retry.props.onClick();
  assert.equal(ui.counts().retries, 1);
  assert.ok(find(ui.dialog(), (element) => element.props?.role === "status"));
  ui.phase("ready");
  const ready = ui.dialog();
  assert.deepEqual(ready.props.style, firstStyle);
  assert.ok(find(ready, (element) => element.props?.className === "assistant-chat-input"));
  assert.equal(find(ready, (element) => element.props?.role === "alert"), null);
});

test("LAN failure can close and a late success cannot reopen it", () => {
  const ui = harness();
  ui.entry.props.onClick();
  ui.phase("error");
  find(ui.dialog(), (element) => element.props?.["aria-label"] === "Close LAN").props.onClick();
  ui.phase("ready");
  assert.equal(ui.dialog(), null);
});

test("a failed recovery keeps LAN closable and explains the limit without offering a dead retry", () => {
  const ui = harness();
  ui.entry.props.onClick();
  ui.phase("error");
  find(ui.dialog(), (element) => element.props?.className === "assistant-load-retry").props.onClick();
  ui.phase("error");
  const failed = ui.dialog();
  assert.ok(find(failed, (element) => element.props?.role === "alert"));
  assert.equal(find(failed, (element) => element.props?.className === "assistant-load-retry"), null);
  assert.ok(find(failed, (element) => element.type === "span" && element.props?.children.includes("reopen the app")));
  find(failed, (element) => element.props?.["aria-label"] === "Close LAN").props.onClick();
  assert.equal(ui.dialog(), null);
  ui.entry.props.onClick();
  assert.equal(find(ui.dialog(), (element) => element.props?.className === "assistant-load-retry"), null);
  assert.equal(ui.counts().retries, 1);
});

test("pointer and keyboard intent preload LAN without opening the dialog", async () => {
  const ui = harness();
  ui.entry.props.onPointerEnter();
  ui.entry.props.onFocus();
  await Promise.resolve();
  assert.equal(ui.counts().preloads, 2);
  assert.equal(ui.dialog(), null);
});
