const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function loadSource(name, dependencies = {}, context = {}) {
  const filename = path.join(__dirname, "../src/components", name);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    Error,
    require: (id) => {
      assert.ok(Object.hasOwn(dependencies, id), `unexpected dependency ${id}`);
      return dependencies[id];
    },
    ...context
  }, { filename });
  return exports;
}

const focus = loadSource("assistant-focus.ts");
const panelPosition = loadSource("assistant-panel-position.ts", {}, { window: { innerWidth: 1560 } });
const aiSettings = loadSource("../ai-settings.ts", {
  "./i18n": { selectLocaleText: (locale, chinese, english) => locale === "zh-CN" ? chinese : english }
});

function componentHarness(name, overrides = {}) {
  const state = [];
  const refs = [];
  let index = 0;
  let refIndex = 0;
  let layoutEffects = [];
  const effects = [];
  let effectIndex = 0;
  const timers = new Map();
  let timerId = 0;
  const node = (type, props) => ({ type, props });
  const dependencies = {
    "react/jsx-runtime": { jsx: node, jsxs: node, Fragment: "fragment" },
    react: {
      useCallback: (callback) => callback,
      useState: (initial) => {
        const position = index++;
        if (!(position in state)) state[position] = initial;
        return [state[position], (next) => {
          state[position] = typeof next === "function" ? next(state[position]) : next;
        }];
      },
      useRef: (initial) => refs[refIndex++] ??= { current: initial },
      useEffect: (callback, dependencies) => {
        const position = effectIndex++;
        const previous = effects[position];
        if (!previous || dependencies.some((value, index) => value !== previous.dependencies[index])) {
          effects[position] = { callback, dependencies, cleanup: previous?.cleanup, pending: true };
        }
      },
      useLayoutEffect: (effect) => layoutEffects.push(effect)
    },
    "react-dom": { createPortal: (tree) => tree },
    "../i18n": {
      useI18n: () => ({ locale: "en-US", t: (key) => key }),
      isChineseLocale: (locale) => locale === "zh-CN"
    },
    "../ai-settings": aiSettings,
    "../api": { listOllamaModels: () => assert.fail("cloud provider must not query Ollama") },
    "../assistant-state": { buildAssistantViewModel: () => ({
      panelTitle: "LAN", closeLabel: "Close LAN", issues: [], prompts: [], contextPayload: "context"
    }) },
    "../hooks/useAssistantPromptRotation": { useAssistantPromptRotation: (prompts = []) => prompts[0] ?? null },
    "./AppAiSettingsCard": {},
    "./AiDataDisclosure": { AiDataDisclosure: "AiDataDisclosure" },
    "./PrivacyNotice": { PrivacyNotice: "PrivacyNotice" },
    "./AppAiConnectionCheck": { AppAiConnectionCheck: "AppAiConnectionCheck" },
    "./KnowledgeSettingsCard": { KnowledgeSettingsCard: "KnowledgeSettingsCard" },
    "./ActivityNotice": { ActivityNotice: "ActivityNotice" },
    "./app-ai-settings.css": {},
    "./AssistantHistoryDrawer": {},
    "./AssistantUpdateNotice": {},
    "./LanMark": {},
    "./ShellIcon": {},
    "./AssistantIslandSurface": { AssistantIslandSurface: "island-surface" },
    "./assistant-focus": focus,
    "./assistant-panel-position": panelPosition,
    "../deferred-module": { createDeferredModule: () => ({ load: () => Promise.resolve(null), peek: () => null }) },
    "../hooks/useDeferredModule": { useDeferredModule: () => ({ status: "ready", value: "loaded-panel", retry() {} }) },
    ...overrides.dependencies
  };
  const exports = loadSource(`${name}.tsx`, dependencies, {
    setTimeout: (callback) => { timers.set(++timerId, callback); return timerId; },
    clearTimeout: (id) => timers.delete(id),
    ...overrides.context
  });
  return {
    render(props) {
      index = 0;
      refIndex = 0;
      effectIndex = 0;
      layoutEffects = [];
      return exports[name](props);
    },
    runLayoutEffects: () => layoutEffects.forEach((effect) => effect()),
    runEffects: () => effects.forEach((effect) => {
      if (effect.pending) {
        effect.pending = false;
        effect.cleanup?.();
        effect.cleanup = effect.callback();
      }
    }),
    runTimers: () => {
      const pending = Array.from(timers.values());
      timers.clear();
      pending.forEach((callback) => callback());
    },
    unmount: () => effects.forEach((effect) => effect.cleanup?.())
  };
}

function findNode(tree, predicate) {
  if (!tree || typeof tree !== "object") return null;
  if (predicate(tree)) return tree;
  const children = tree.props?.children;
  for (const child of Array.isArray(children) ? children.flat(Infinity) : [children]) {
    const found = findNode(child, predicate);
    if (found) return found;
  }
  return null;
}

function keyboard(overrides = {}) {
  return {
    key: "Enter", shiftKey: false, nativeEvent: { isComposing: false },
    prevented: false, stopped: false,
    preventDefault() { this.prevented = true; },
    stopPropagation() { this.stopped = true; },
    ...overrides
  };
}

function panelProps(overrides = {}) {
  return {
    assistantInput: { storageReady: true, selectedInstanceId: null, selectedModuleId: null, bootstrap: { state: { modules: [] } } }, draft: "  检查服务器  ",
    aiSettings: { ...aiSettings.createDefaultAiSettings(), enabled: true, model: "test-model", apiKeyStored: true },
    appUpdateState: { status: "idle" },
    execution: { status: "idle", promptLabel: null }, messages: [], conversations: [],
    onReady: () => undefined, onDraftChange: () => undefined, onSendMessage: () => undefined,
    ...overrides
  };
}

test("composer accepts natural-language requests without requiring goal or target controls", () => {
  for (const draft of ["把人数改成 12", "这个服进不去，帮我修好", "帮我新建一个饥荒服"]) {
    const calls = [];
    const tree = componentHarness("AssistantPanel").render(panelProps({
      draft,
      onDraftChange: (value) => calls.push(["draft", value]),
      onSendMessage: (...args) => calls.push(["send", ...args]),
    }));
    assert.equal(findNode(tree, (node) => node.type === "select"), null);
    assert.equal(findNode(tree, (node) => node.type === "input" && node.props.type === "checkbox"), null);
    assert.equal(findNode(tree, (node) => node.props?.className?.includes("assistant-inline-button")).props.disabled, false);
    findNode(tree, (node) => node.type === "textarea").props.onKeyDown(keyboard());
    assert.deepEqual(calls, [["draft", ""], ["send", draft]]);
  }
});

test("LAN composer preserves Chinese composition and Shift+Enter before submitting a finished message", () => {
  const calls = [];
  const harness = componentHarness("AssistantPanel");
  const tree = harness.render(panelProps({
    onDraftChange: (value) => calls.push(["draft", value]),
    onSendMessage: (...args) => calls.push(["send", ...args])
  }));
  const input = findNode(tree, (node) => node.type === "textarea");
  for (const event of [keyboard({ nativeEvent: { isComposing: true } }), keyboard({ shiftKey: true })]) {
    input.props.onKeyDown(event);
    assert.equal(event.prevented, false);
    assert.equal(calls.length, 0);
  }
  const enter = keyboard();
  input.props.onKeyDown(enter);
  assert.equal(enter.prevented, true);
  assert.deepEqual(calls, [["draft", ""], ["send", "检查服务器"]]);
});

test("an active assistant request preserves the next draft and disables sending", () => {
  const tree = componentHarness("AssistantPanel").render(panelProps({
    execution: { status: "running", promptLabel: "Diagnosis" },
    onDraftChange: () => assert.fail("must preserve the next draft"),
    onSendMessage: () => assert.fail("must not submit while running")
  }));
  findNode(tree, (node) => node.type === "textarea").props.onKeyDown(keyboard());
  assert.equal(findNode(tree, (node) => node.props?.className?.includes("assistant-inline-button")).props.disabled, true);
});

test("multiline drafts grow to the composer height limit and shrink after clearing", () => {
  const harness = componentHarness("AssistantPanel");
  const input = { style: {}, scrollHeight: 240 };
  let tree = harness.render(panelProps());
  findNode(tree, (node) => node.type === "textarea").props.ref.current = input;
  harness.runLayoutEffects();
  assert.equal(input.style.height, "120px");
  input.scrollHeight = 32;
  tree = harness.render(panelProps({ draft: "" }));
  harness.runLayoutEffects();
  assert.equal(input.style.height, "32px");
  assert.equal(findNode(tree, (node) => node.props?.role === "log").props["aria-live"], "polite");
});

test("an unconfigured model disables sending and explains why on the send button", () => {
  const harness = componentHarness("AssistantPanel");
  const props = panelProps({
    aiSettings: aiSettings.createDefaultAiSettings(),
    onDraftChange: () => assert.fail("an unconfigured model must preserve the draft"),
    onSendMessage: () => assert.fail("an unconfigured model must not receive a request")
  });
  let tree = harness.render(props);
  findNode(tree, (node) => node.type === "textarea").props.onKeyDown(keyboard());
  const send = findNode(tree, (node) => node.props?.className?.includes("assistant-inline-button"));
  assert.equal(send.props.disabled, true);
  assert.equal(findNode(tree, (node) => node.props?.className === "assistant-send-control").props.title, "assistant.run.notReady");
  assert.equal(send.props["aria-label"], "assistant.run.notReady");
  assert.equal(findNode(tree, (node) => node.props?.className === "assistant-setup-action"), null);
  assert.equal(findNode(tree, (node) => node.props?.className === "assistant-panel-title"), null);
  assert.equal(findNode(tree, (node) => node.props?.className === "assistant-chat-message-body").props.children, "assistant.chat.greeting");
  assert.equal(findNode(tree, (node) => node.type === "textarea").props.placeholder, "assistant.chat.inputPlaceholder");
  findNode(tree, (node) => node.props?.className === "assistant-settings-button").props.onClick();
  tree = harness.render(props);
  assert.ok(findNode(tree, (node) => node.props?.className === "assistant-ai-settings-surface"));
  assert.equal(findNode(tree, (node) => node.props?.className === "assistant-panel-title").props.children, "assistant.panel.settingsLabel");
  findNode(tree, (node) => node.props?.className === "assistant-back-button").props.onClick();
  tree = harness.render(props);
  assert.equal(findNode(tree, (node) => node.type === "textarea").props.value, props.draft);
});

test("composer placeholder uses rotating suggestions instead of a carousel", () => {
  const tree = componentHarness("AssistantPanel", {
    dependencies: {
      "../assistant-state": { buildAssistantViewModel: () => ({
        panelTitle: "LAN", closeLabel: "Close LAN", issues: [],
        prompts: [{ id: "logs", label: "Check logs", preview: "preview", prompt: "prompt", payload: "payload" }],
        contextPayload: "context"
      }) }
    }
  }).render(panelProps({ draft: "" }));
  assert.equal(findNode(tree, (node) => node.type === "textarea").props.placeholder, "preview");
  assert.equal(findNode(tree, (node) => node.props?.className === "assistant-prompt-carousel"), null);
  assert.equal(findNode(tree, (node) => node.props?.className?.includes("assistant-chat-empty")), null);
});

test("new replies preserve the reader's scroll position and latest-message navigation resumes following", () => {
  const harness = componentHarness("AssistantPanel");
  const feed = { scrollHeight: 2400, clientHeight: 500, scrollTop: 0 };
  const props = panelProps({ activeConversationId: "one", messages: [{ id: "initial", role: "user", content: "Inspect current resources" }] });
  let tree = harness.render(props);
  findNode(tree, (node) => node.props?.role === "log").props.ref.current = feed;
  harness.runEffects();
  assert.equal(feed.scrollTop, 2400);

  feed.scrollTop = 180;
  findNode(tree, (node) => node.props?.role === "log").props.onScroll();
  const updated = { ...props, messages: [{ id: "reply", role: "assistant", content: "A new reply" }] };
  tree = harness.render(updated);
  harness.runEffects();
  assert.equal(feed.scrollTop, 180);
  findNode(tree, (node) => node.props?.className === "assistant-chat-jump").props.onClick();
  assert.equal(feed.scrollTop, 2400);

  feed.scrollHeight = 2600;
  tree = harness.render({ ...updated, execution: { status: "running", promptLabel: "Next answer" } });
  harness.runEffects();
  assert.equal(feed.scrollTop, 2600);
  feed.scrollTop = 180;
  findNode(tree, (node) => node.props?.role === "log").props.onScroll();
  harness.render({ ...props, activeConversationId: "two" });
  harness.runEffects();
  assert.equal(feed.scrollTop, 2600, "switching conversations should show the selected conversation's latest messages");
});

test("an empty conversation starts with LAN's greeting and does not offer latest-message navigation", () => {
  const harness = componentHarness("AssistantPanel");
  const feed = { scrollHeight: 500, clientHeight: 300, scrollTop: 200 };
  const props = panelProps();
  let tree = harness.render(props);
  assert.equal(findNode(tree, (node) => node.props?.className === "assistant-panel-title"), null);
  assert.equal(findNode(tree, (node) => node.props?.className === "assistant-panel-context"), null);
  const greeting = findNode(tree, (node) => node.props?.className === "assistant-chat-message is-assistant");
  assert.ok(findNode(greeting, (node) => node.props?.className === "assistant-chat-avatar is-mascot"));
  assert.ok(findNode(greeting, (node) => node.props?.className === "assistant-chat-main"));
  assert.equal(findNode(greeting, (node) => node.type === "strong").props.children, "assistant.chat.assistantName");
  assert.equal(findNode(greeting, (node) => node.props?.className === "assistant-chat-message-body").props.children, "assistant.chat.greeting");
  findNode(tree, (node) => node.props?.role === "log").props.ref.current = feed;
  harness.runEffects();
  assert.equal(feed.scrollTop, 0);
  findNode(tree, (node) => node.props?.role === "log").props.onScroll();
  tree = harness.render(props);
  assert.equal(findNode(tree, (node) => node.props?.className === "assistant-chat-jump"), null);
});

test("a started conversation keeps LAN's greeting above the transcript", () => {
  const tree = componentHarness("AssistantPanel").render(panelProps({
    messages: [{ id: "user-1", role: "user", content: "帮我开服" }]
  }));
  const greeting = findNode(tree, (node) => node.props?.className === "assistant-chat-message is-assistant");
  assert.equal(findNode(greeting, (node) => node.props?.className === "assistant-chat-message-body").props.children, "assistant.chat.greeting");
  const user = findNode(tree, (node) => node.props?.className === "assistant-chat-message is-user");
  assert.ok(findNode(user, (node) => node.props?.className === "assistant-chat-avatar is-user"));
  assert.equal(findNode(user, (node) => node.props?.className === "assistant-chat-message-head"), null);
  assert.equal(findNode(user, (node) => node.props?.className === "assistant-chat-message-body").props.children, "帮我开服");
});

test("a running reply shows a typing bubble instead of a processing status", () => {
  const tree = componentHarness("AssistantPanel").render(panelProps({
    execution: { status: "running", promptLabel: "帮我开服" },
    messages: [{ id: "user-1", role: "user", content: "帮我开服" }]
  }));
  const typing = findNode(tree, (node) => node.props?.className === "assistant-chat-message is-assistant is-running");
  assert.ok(findNode(typing, (node) => node.props?.className === "assistant-chat-typing"));
  assert.equal(findNode(typing, (node) => node.props?.className === "assistant-chat-bubble").props["aria-label"], "assistant.chat.runningLabel");
  assert.equal(findNode(typing, (node) => node.props?.className === "assistant-chat-message-body"), null);
  assert.equal(findNode(typing, (node) => node.type === "strong").props.children, "assistant.chat.assistantName");
});

test("the main header can start a conversation and returns focus to the composer", () => {
  const harness = componentHarness("AssistantPanel");
  let created = 0;
  let focused = 0;
  const props = panelProps({ onNewConversation: () => { created += 1; } });
  let tree = harness.render(props);
  findNode(tree, (node) => node.type === "textarea").props.ref.current = { focus: () => { focused += 1; } };
  findNode(tree, (node) => node.props?.className === "assistant-new-button").props.onClick();
  assert.equal(created, 1);
  assert.equal(focused, 1);
  tree = harness.render({ ...props, execution: { status: "running", promptLabel: "Busy" } });
  const create = findNode(tree, (node) => node.props?.className === "assistant-new-button");
  assert.equal(create.props.disabled, true);
  create.props.onClick();
  assert.equal(created, 1);
});

test("history retains an explicit close button while a request disables conversation changes", () => {
  let closed = 0;
  const tree = componentHarness("AssistantHistoryDrawer").render({
    open: true, disabled: true, conversations: [], onClose: () => { closed += 1; }
  });
  const close = findNode(tree, (node) => node.props?.className === "assistant-history-close");
  assert.equal(Boolean(close.props.disabled), false);
  close.props.onClick();
  assert.equal(closed, 1);
});

function focusTarget(options = {}) {
  return { tabIndex: 0, focused: false, closest: () => null, focus() { this.focused = true; }, ...options };
}

function focusContainer(elements, input = null) {
  return focusTarget({
    querySelectorAll: () => elements, querySelector: () => input,
    contains: (element) => elements.includes(element)
  });
}

function openedCapsule(capsuleRect = null) {
  const handlers = new Map();
  const frames = [];
  const trigger = focusTarget({
    getBoundingClientRect: () => capsuleRect,
    closest: () => ({ getBoundingClientRect: () => ({ left: 0, top: 0, width: 1560, height: 86, bottom: 86 }) })
  });
  const document = {
    body: {}, activeElement: trigger,
    addEventListener: (name, handler) => handlers.set(name, handler),
    removeEventListener: (name) => handlers.delete(name)
  };
  const harness = componentHarness("AssistantCapsule", { context: {
    document,
    window: {
      addEventListener() {}, removeEventListener() {},
      requestAnimationFrame: (callback) => frames.push(callback), cancelAnimationFrame() {}
    }
  } });
  const props = { assistant: { panelTitle: "LAN", tone: "idle" }, appUpdateState: { status: "idle" } };
  let tree = harness.render(props);
  findNode(tree, (node) => node.type === "button").props.ref.current = trigger;
  findNode(tree, (node) => node.type === "button").props.onClick();
  tree = harness.render(props);
  tree.props.ref.current = focusContainer([trigger]);
  harness.runEffects();
  return { harness, props, handlers, frames, trigger };
}

test("clicking outside LAN dismisses it without reclaiming the clicked control's focus", () => {
  const capsule = openedCapsule();
  capsule.handlers.get("mousedown")({ target: focusTarget() });
  const tree = capsule.harness.render(capsule.props);
  assert.equal(findNode(tree, (node) => node.type === "button").props["aria-expanded"], false);
  assert.equal(capsule.frames.length, 0);
  assert.equal(capsule.trigger.focused, false);
  capsule.harness.unmount();
});

test("an operation modal suspends the capsule keyboard and outside-click handlers until it closes", () => {
  const capsule = openedCapsule();
  assert.equal(capsule.handlers.has("keydown"), true);
  capsule.harness.render({ ...capsule.props, confirmationOpen: true });
  capsule.harness.runEffects();
  assert.equal(capsule.handlers.has("keydown"), false);
  assert.equal(capsule.handlers.has("mousedown"), false);
  const tree = capsule.harness.render(capsule.props);
  capsule.harness.runEffects();
  assert.equal(findNode(tree, (node) => node.type === "button").props["aria-expanded"], true);
  assert.equal(capsule.handlers.has("keydown"), true);
  assert.equal(capsule.handlers.has("mousedown"), true);
  capsule.harness.unmount();
});

test("LAN expands from the capsule's own top edge and center, independently of the header", () => {
  const origin = { left: 897, top: 6, width: 126, height: 37, bottom: 43 };
  const capsule = openedCapsule(origin);
  const tree = capsule.harness.render(capsule.props);
  const panel = findNode(tree, (node) => node.props?.anchorRect);
  assert.equal(panel.props.anchorRect, origin);
  const mountedPanel = componentHarness("AssistantPanel", { context: { window: { innerWidth: 1560 } } })
    .render(panelProps({ anchorRect: panel.props.anchorRect }));
  const style = mountedPanel.props.style;
  assert.equal(parseFloat(style["--assistant-panel-top"]), origin.top);
  assert.equal(parseFloat(style["--assistant-panel-left"]) + parseFloat(style["--assistant-panel-width"]) / 2,
    origin.left + origin.width / 2);
  assert.ok(parseFloat(style["--assistant-panel-left"]) + parseFloat(style["--assistant-panel-width"]) <= 1560);
  capsule.harness.unmount();
});

test("Escape cancels composition without closing LAN, then explicit Escape restores entry focus", () => {
  const capsule = openedCapsule();
  const composing = keyboard({ key: "Escape", isComposing: true });
  capsule.handlers.get("keydown")(composing);
  let tree = capsule.harness.render(capsule.props);
  assert.equal(findNode(tree, (node) => node.type === "button").props["aria-expanded"], true);
  assert.equal(composing.prevented, false);
  capsule.handlers.get("keydown")(keyboard({ key: "Escape" }));
  tree = capsule.harness.render(capsule.props);
  assert.equal(findNode(tree, (node) => node.type === "button").props["aria-expanded"], false);
  capsule.frames.forEach((callback) => callback());
  assert.equal(capsule.trigger.focused, true);
  capsule.harness.unmount();
});

test("opening LAN focuses the composer with a loading-panel fallback", () => {
  const settings = focusTarget();
  const input = focusTarget();
  focus.focusAssistantPanel(focusContainer([settings, input], input));
  assert.equal(input.focused, true);
  assert.equal(settings.focused, false);
  const fallback = focusContainer([]);
  focus.focusAssistantPanel(fallback);
  assert.equal(fallback.focused, true);
});

test("assistant focus skips hidden subtrees and negative tabindex, and wraps from the dialog root", () => {
  const first = focusTarget();
  const hidden = focusTarget({ closest: () => ({ hidden: true }) });
  const scrim = focusTarget({ tabIndex: -1 });
  const last = focusTarget();
  const panel = focusContainer([first, hidden, scrim, last]);
  assert.deepEqual(Array.from(focus.listAssistantFocusableElements(panel)), [first, last]);
  for (const [active, shiftKey, expected] of [[panel, false, first], [first, true, last], [last, false, first]]) {
    const event = keyboard({ key: "Tab", shiftKey });
    focus.containAssistantFocus(event, panel, active);
    assert.equal(event.prevented, true);
    assert.equal(expected.focused, true);
    first.focused = last.focused = false;
  }
});

test("history Escape cancels deletion before closing history and does not close LAN", () => {
  const harness = componentHarness("AssistantHistoryDrawer");
  let closed = 0;
  const props = {
    open: true, disabled: false, activeConversationId: "one",
    conversations: [{ id: "one", title: "Diagnosis", updatedAt: 1000 }],
    onClose: () => { closed += 1; }, onDelete: () => assert.fail("cancel must not delete")
  };
  let tree = harness.render(props);
  findNode(tree, (node) => node.props?.className === "assistant-history-delete").props.onClick();
  tree = harness.render(props);
  const firstEscape = keyboard({ key: "Escape" });
  findNode(tree, (node) => node.type === "aside").props.onKeyDown(firstEscape);
  assert.equal(firstEscape.stopped, true);
  assert.equal(firstEscape.prevented, true);
  assert.equal(closed, 0);
  tree = harness.render(props);
  assert.equal(findNode(tree, (node) => node.props?.className === "assistant-history-confirm"), null);
  findNode(tree, (node) => node.type === "aside").props.onKeyDown(keyboard({ key: "Escape" }));
  assert.equal(closed, 1);
});

test("a running request disables an already-open conversation deletion confirmation", () => {
  const harness = componentHarness("AssistantHistoryDrawer");
  const props = {
    open: true, disabled: false, activeConversationId: "one",
    conversations: [{ id: "one", title: "Diagnosis", updatedAt: 1000 }],
    onDelete: () => assert.fail("in-flight conversation must not be deleted")
  };
  let tree = harness.render(props);
  findNode(tree, (node) => node.props?.className === "assistant-history-delete").props.onClick();
  tree = harness.render({ ...props, disabled: true });
  const confirm = findNode(tree, (node) => node.props?.className === "is-danger");
  assert.equal(confirm.props.disabled, true);
  confirm.props.onClick();
});

test("canceling history deletion restores its row focus; completed or external removal finds a surviving target", () => {
  for (const action of ["cancel", "Escape", "delete", "external", "last-delete"]) {
    let focused;
    const harness = componentHarness("AssistantHistoryDrawer", { context: { document: { activeElement: null } } });
    const props = {
      open: true, disabled: false, activeConversationId: "one",
      conversations: [{ id: "one", title: "First", updatedAt: 1000 }, { id: "two", title: "Second", updatedAt: 2000 }],
      onDelete: (id) => { props.conversations = props.conversations.filter((item) => item.id !== id); }
    };
    if (action === "last-delete") props.conversations = props.conversations.slice(0, 1);
    const button = (kind, id) => focusTarget({
      dataset: { conversationId: id }, classList: { contains: (value) => value === `assistant-history-${kind}` },
      focus: () => { focused = `${kind}:${id ?? ""}`; }
    });
    const drawer = focusContainer([]);
    drawer.querySelectorAll = () => [button("close"), ...props.conversations.flatMap(({ id }) => [button("select", id), button("delete", id)])];
    let tree = harness.render(props);
    findNode(tree, (node) => node.type === "aside").props.ref.current = drawer;
    findNode(tree, (node) => node.props?.className === "assistant-history-delete").props.onClick();
    tree = harness.render(props);
    if (action === "cancel") {
      findNode(tree, (node) => node.type === "button" && node.props.children === "assistant.history.cancel").props.onClick();
    } else if (action === "Escape") {
      findNode(tree, (node) => node.type === "aside").props.onKeyDown(keyboard({ key: "Escape" }));
    } else if (action === "external") {
      props.conversations = props.conversations.slice(1);
    } else {
      findNode(tree, (node) => node.props?.className === "is-danger").props.onClick();
    }
    harness.render(props);
    harness.runLayoutEffects();
    assert.equal(focused, action === "cancel" || action === "Escape" ? "delete:one" : action === "last-delete" ? "close:" : "select:two", action);
  }
});

test("history leaves an active composition and pending deletion intact on Escape", () => {
  const harness = componentHarness("AssistantHistoryDrawer");
  const props = {
    open: true, disabled: false, conversations: [{ id: "one", title: "First", updatedAt: 1000 }],
    onClose: () => assert.fail("composition must not close history")
  };
  let tree = harness.render(props);
  findNode(tree, (node) => node.props?.className === "assistant-history-delete").props.onClick();
  tree = harness.render(props);
  const event = keyboard({ key: "Escape", nativeEvent: { isComposing: true } });
  findNode(tree, (node) => node.type === "aside").props.onKeyDown(event);
  assert.equal(event.prevented, false);
  tree = harness.render(props);
  assert.ok(findNode(tree, (node) => node.props?.className === "assistant-history-confirm"));
});

test("an old autosave response cannot replace model text entered during persistence", async () => {
  const harness = componentHarness("AppAiSettingsCard");
  const initial = { ...aiSettings.createDefaultAiSettings(), model: "initial" };
  const writes = [];
  let finishFirst;
  const props = {
    settings: initial,
    onClearSecret: () => assert.fail("must not clear credentials"),
    onSave: (settings) => {
      writes.push(settings);
      return new Promise((resolve) => { finishFirst = resolve; });
    }
  };
  const modelInput = (tree) => findNode(tree, (node) => node.type === "input" && node.props.placeholder === "Model");
  let tree = harness.render(props);
  harness.runEffects();
  modelInput(tree).props.onChange({ target: { value: "model-a" } });
  harness.render(props);
  harness.runEffects();
  harness.runTimers();
  assert.equal(writes[0].model, "model-a");

  tree = harness.render(props);
  modelInput(tree).props.onChange({ target: { value: "model-b" } });
  harness.render(props);
  harness.runEffects();
  const persisted = { ...writes[0], apiKey: "" };
  const updatedProps = { ...props, settings: persisted };
  harness.render(updatedProps);
  harness.runEffects();
  finishFirst(persisted);
  await new Promise((resolve) => setImmediate(resolve));
  tree = harness.render(updatedProps);
  assert.equal(modelInput(tree).props.value, "model-b");
  harness.runEffects();
  harness.runTimers();
  assert.equal(writes.at(-1).model, "model-b");
});

test("a failed key clear keeps the stored-key state and offers the matching retry action", async () => {
  const harness = componentHarness("AppAiSettingsCard");
  let attempts = 0;
  const props = {
    settings: { ...aiSettings.createDefaultAiSettings(), model: "selected", apiKeyStored: true },
    onSave: () => assert.fail("retrying a key clear must not save settings"),
    onClearSecret: async () => { attempts += 1; throw new Error("Credential store unavailable"); }
  };
  let tree = harness.render(props);
  harness.runEffects();
  const clear = findNode(tree, (node) => node.type === "button" && findNode(node, (child) => child.props?.children === "Clear key"));
  clear.props.onClick();
  await new Promise((resolve) => setImmediate(resolve));
  tree = harness.render(props);
  assert.equal(findNode(tree, (node) => node.props?.type === "password").props.placeholder, "••••••••••••");
  const alert = findNode(tree, (node) => node.type === "ActivityNotice" && node.props.tone === "error");
  assert.equal(alert.props.children, "Could not clear key: Credential store unavailable");
  const retry = alert.props.action;
  assert.equal(retry.props.children, "Retry clear");
  retry.props.onClick();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(attempts, 2);
});

test("closing AI settings before the debounce expires saves the final draft exactly once", async () => {
  const harness = componentHarness("AppAiSettingsCard");
  const writes = [];
  const props = {
    settings: { ...aiSettings.createDefaultAiSettings(), model: "initial" },
    onSave: async (settings) => { writes.push(settings); return settings; }
  };
  let tree = harness.render(props);
  harness.runEffects();
  findNode(tree, (node) => node.type === "input" && node.props.placeholder === "Model")
    .props.onChange({ target: { value: "final-model" } });
  harness.render(props);
  harness.runEffects();
  assert.equal(writes.length, 0);
  harness.unmount();
  harness.runTimers();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(writes.length, 1);
  assert.equal(writes[0].model, "final-model");
});

test("AI connection fields have individual labels in provider, address, model and key order", () => {
  const tree = componentHarness("AppAiSettingsCard").render({ settings: aiSettings.createDefaultAiSettings() });
  const fieldIds = [];
  const collect = (node) => {
    if (!node || typeof node !== "object") return;
    if (node.type === "label") {
      fieldIds.push(node.props.htmlFor);
      assert.ok(findNode(tree, (field) => field.props?.id === node.props.htmlFor));
      assert.equal(findNode(node, (child) => ["input", "select", "button"].includes(child.type)), null);
    }
    for (const child of [node.props?.children].flat(Infinity)) collect(child);
  };
  collect(tree);
  assert.deepEqual(fieldIds, ["ai-settings-provider", "ai-settings-base-url", "ai-settings-model", "ai-settings-api-key"]);
  assert.equal(findNode(tree, (node) => node.props?.id === "ai-settings-api-key").props["aria-describedby"], "ai-settings-key-note");
});

function aiSaveStatus(tree) {
  return findNode(tree, (node) => node.type === "ActivityNotice" && node.props.tone !== "error");
}

test("AI autosave acknowledges success only after persistence resolves", async () => {
  const harness = componentHarness("AppAiSettingsCard");
  let resolveSave;
  let submitted;
  const props = {
    settings: { ...aiSettings.createDefaultAiSettings(), model: "initial" },
    onSave: (settings) => { submitted = settings; return new Promise((resolve) => { resolveSave = resolve; }); }
  };
  let tree = harness.render(props);
  harness.runEffects();
  findNode(tree, (node) => node.props?.id === "ai-settings-model").props.onChange({ target: { value: "chosen-model" } });
  tree = harness.render(props);
  harness.runEffects();
  assert.equal(aiSaveStatus(tree).props.children, "Waiting to save…");
  assert.equal(findNode(tree, (node) => node.type === "AppAiConnectionCheck").props.disabled, true);
  assert.equal(submitted, undefined);
  harness.runTimers();
  tree = harness.render(props);
  assert.equal(aiSaveStatus(tree).props.children, "Saving…");
  assert.equal(findNode(tree, (node) => node.type === "AppAiConnectionCheck").props.disabled, true);
  resolveSave(submitted);
  await new Promise((resolve) => setImmediate(resolve));
  tree = harness.render(props);
  assert.equal(aiSaveStatus(tree).props.tone, "success");
  assert.equal(aiSaveStatus(tree).props.children, "Saved on this device");
  assert.equal(findNode(tree, (node) => node.type === "AppAiConnectionCheck").props.disabled, false);
  assert.ok(findNode(aiSaveStatus(tree), (node) => node.props?.children === "Saved on this device"));
});

test("an earlier save cannot acknowledge a newer unsaved draft", async () => {
  const harness = componentHarness("AppAiSettingsCard");
  let resolveSave;
  let submitted;
  const props = {
    settings: { ...aiSettings.createDefaultAiSettings(), model: "initial" },
    onSave: (settings) => { submitted = settings; return new Promise((resolve) => { resolveSave = resolve; }); }
  };
  let tree = harness.render(props);
  harness.runEffects();
  findNode(tree, (node) => node.props?.id === "ai-settings-model").props.onChange({ target: { value: "model-a" } });
  harness.render(props);
  harness.runEffects();
  harness.runTimers();
  tree = harness.render(props);
  findNode(tree, (node) => node.props?.id === "ai-settings-model").props.onChange({ target: { value: "model-b" } });
  harness.render(props);
  harness.runEffects();
  resolveSave(submitted);
  await new Promise((resolve) => setImmediate(resolve));
  tree = harness.render(props);
  assert.equal(aiSaveStatus(tree).props.children, "Waiting to save…");
  assert.equal(findNode(tree, (node) => node.props?.id === "ai-settings-model").props.value, "model-b");
});

test("returning to persisted AI values cancels pending feedback and the queued save", () => {
  const harness = componentHarness("AppAiSettingsCard");
  const props = {
    settings: { ...aiSettings.createDefaultAiSettings(), model: "initial" },
    onSave: () => assert.fail("the reverted draft must not be saved")
  };
  let tree = harness.render(props);
  harness.runEffects();
  for (const value of ["changed", "initial"]) {
    findNode(tree, (node) => node.props?.id === "ai-settings-model").props.onChange({ target: { value } });
    tree = harness.render(props);
    harness.runEffects();
  }
  harness.runTimers();
  tree = harness.render(props);
  assert.equal(aiSaveStatus(tree).props.children, "Changes save automatically");
});

test("an unacknowledged superseded write does not claim that AI settings were saved", async () => {
  const harness = componentHarness("AppAiSettingsCard");
  const props = {
    settings: { ...aiSettings.createDefaultAiSettings(), model: "initial" },
    onSave: async () => null
  };
  let tree = harness.render(props);
  harness.runEffects();
  findNode(tree, (node) => node.props?.id === "ai-settings-model").props.onChange({ target: { value: "new-model" } });
  harness.render(props);
  harness.runEffects();
  harness.runTimers();
  await new Promise((resolve) => setImmediate(resolve));
  tree = harness.render(props);
  assert.equal(aiSaveStatus(tree).props.children, "Changes save automatically");
});

test("a failed AI save preserves the draft and its retry persists the same values", async () => {
  const harness = componentHarness("AppAiSettingsCard");
  const writes = [];
  const props = {
    settings: { ...aiSettings.createDefaultAiSettings(), model: "initial" },
    onSave: async (settings) => {
      writes.push(settings);
      if (writes.length === 1) throw new Error("Settings storage unavailable");
      return settings;
    }
  };
  let tree = harness.render(props);
  harness.runEffects();
  findNode(tree, (node) => node.props?.id === "ai-settings-model").props.onChange({ target: { value: "chosen-model" } });
  harness.render(props);
  harness.runEffects();
  harness.runTimers();
  await new Promise((resolve) => setImmediate(resolve));
  tree = harness.render(props);
  const alert = findNode(tree, (node) => node.type === "ActivityNotice" && node.props.tone === "error");
  assert.equal(alert.props.children, "Save failed: Settings storage unavailable");
  assert.equal(findNode(tree, (node) => node.props?.id === "ai-settings-model").props.value, "chosen-model");
  assert.equal(findNode(tree, (node) => node.type === "AppAiConnectionCheck").props.disabled, false);
  alert.props.action.props.onClick();
  harness.render(props);
  harness.runEffects();
  harness.runTimers();
  await new Promise((resolve) => setImmediate(resolve));
  tree = harness.render(props);
  assert.equal(writes.length, 2);
  assert.deepEqual(writes[1], writes[0]);
  assert.equal(findNode(tree, (node) => node.type === "ActivityNotice" && node.props.tone === "error"), null);
  assert.equal(aiSaveStatus(tree).props.tone, "success");
  assert.equal(aiSaveStatus(tree).props.children, "Saved on this device");
});
