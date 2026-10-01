const assert = require("node:assert/strict");
const test = require("node:test");
const { loadAppHandler } = require("./helpers/assistant-app-handler.cjs");

function deferred() {
  let resolve, reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function reply(overrides = {}) {
  return { conversationId: "fixture-conversation", conversationRevision: 2, handled: false, action: "none",
    message: "Actual reply", continuation: null, requiresConfirmation: false,
    appliedSettingsKeys: [], appliedPortNames: [], workshopItemIds: [], resolvedModIds: [],
    sourcePaths: [], runtimeCommands: [], ...overrides };
}

function attachStore(handler, checkpoint = null) {
  const api = handler.context;
  const settingsIdentity = api.assistantConversationProviderIdentity(api.aiSettings);
  let store = api.beginAssistantConversationTurn(api.createEmptyAssistantConversationStore(), "original-scope",
    { id: "previous-user", role: "user", content: "Previous request" }, 1, () => "display-old", settingsIdentity).store;
  store = api.bindAssistantBackendConversation(store, "display-old", { conversationId: "fixture-conversation", revision: 0 });
  store = api.setAssistantConversationContinuation(store, "display-old", checkpoint);
  let sequence = 0;
  api.assistantConversations = {
    get activeConversation() { return store.conversations.find((entry) => entry.id === store.activeConversationByScope["original-scope"]); },
    beginTurn(message, identity) {
      const turn = api.beginAssistantConversationTurn(store, "original-scope", message, 2, () => `display-new-${++sequence}`, identity);
      store = turn.store;
      return turn;
    },
    bindBackend: (id, binding) => { store = api.bindAssistantBackendConversation(store, id, binding); },
    markUnavailable: (id, unsentId) => { store = api.markAssistantConversationUnavailable(store, id, unsentId); },
    setContinuation: (id, value) => { store = api.setAssistantConversationContinuation(store, id, value); },
    setRecoveryPending: (id, value) => { store = api.setAssistantConversationRecoveryPending(store, id, value); },
    appendMessage: (id, value) => { store = api.appendAssistantConversationMessage(store, id, { id: `reply-${++sequence}`, ...value }); },
    backendId: (id) => store.conversations.find((entry) => entry.id === id)?.backendConversationId,
    remove: (id) => { store = api.assistantConversationStore.deleteAssistantConversation(store, "original-scope", id); return true; },
  };
  return () => store;
}

for (const cancelFails of [false, true]) {
  test(`App keeps the turn locked after a request error until ${cancelFails ? "failed" : "successful"} cancellation settles`, async () => {
    const entered = deferred(), request = deferred(), cancel = deferred();
    const handler = loadAppHandler(() => { entered.resolve(); return request.promise; }, [], [], {}, {
      cancelAssistantTurn: () => cancel.promise,
    });
    const running = handler.run();
    await entered.promise;
    const stopping = handler.stop();
    request.reject(new Error("response lost"));
    await new Promise(setImmediate);
    assert.equal(handler.context.assistantRequestRef.current, true);
    assert.equal(handler.states.at(-1).status, "running");
    assert.equal(handler.states.at(-1).stopping, true);
    await handler.run();
    assert.equal(handler.requests.length, 1, "A second turn must not overlap the old cancellation");
    if (cancelFails) cancel.reject(new Error("cancel transport failed"));
    else cancel.resolve({ conversationId: "fixture-conversation", stopping: true });
    await Promise.all([running, stopping]);
    assert.equal(handler.context.assistantRequestRef.current, false);
    assert.equal(handler.context.assistantTurnRef.current, null);
    assert.equal(handler.states.at(-1).status, cancelFails ? "error" : "inconclusive");
    if (!cancelFails) assert.equal(handler.states.at(-1).error, "assistant.session.stopUnconfirmed");
  });
}

test("App replaces an unavailable backend binding and preserves only the old displayed history", async () => {
  let creates = 0;
  const handler = loadAppHandler(reply({ conversationId: "fresh-backend" }), [], [], {}, {
    getAssistantConversationState: async (conversationId) => ({ conversationId, status: "unavailable", revision: null, continuation: null }),
    createAssistantConversation: async () => { creates += 1; return { conversationId: "fresh-backend", revision: 0 }; },
  });
  const read = attachStore(handler);
  await handler.run();
  assert.equal(creates, 1);
  assert.deepEqual(handler.requests.map((request) => request.conversationId), ["fresh-backend"]);
  assert.equal(read().conversations.length, 2);
  const old = read().conversations.find((entry) => entry.id === "display-old");
  assert.equal(old.unavailable, true);
  assert.equal(old.messages[0].content, "Previous request");
  assert.equal(old.messages.some((entry) => entry.id === "current-user"), false, "Unsent message belongs to the new conversation");
  assert.equal(handler.context.assistantConversations.activeConversation.backendConversationId, "fresh-backend");
  assert.equal(Object.hasOwn(handler.requests[0], "conversationMessages"), false);
});

test("unknown preflight status never sends a model request and retains a read-only recovery entry", async () => {
  const handler = loadAppHandler(() => assert.fail("Unknown state must not send a new request"), [], [], {}, {
    getAssistantConversationState: async () => { throw new Error("connection unavailable"); },
  });
  attachStore(handler);
  await handler.run();
  assert.equal(handler.context.assistantConversations.activeConversation.recoveryPending, true);
  assert.equal(handler.context.assistantConversations.activeConversation.unavailable, false);
  assert.equal(handler.requests.length, 0);
});

test("an already-running backend turn blocks another prompt and preserves its read-only recovery entry", async () => {
  const handler = loadAppHandler(() => assert.fail("The previous turn is still running"), [], [], {}, {
    getAssistantConversationState: async (conversationId) => ({ conversationId, status: "running", revision: 3, continuation: null }),
  });
  attachStore(handler);
  await handler.run();
  assert.equal(handler.context.assistantConversations.activeConversation.recoveryPending, true);
  assert.equal(handler.states.at(-1).status, "inconclusive");
  assert.equal(handler.requests.length, 0);
});

test("a failed recovery-state check stays available and cannot consume or resend its saved checkpoint", async () => {
  const handler = loadAppHandler(() => assert.fail("State inspection must not execute"), [], [], {}, {
    getAssistantConversationState: async () => { throw new Error("offline"); },
    resumeAssistantConversation: () => assert.fail("State inspection must not resume"),
  });
  const checkpoint = { reason: "model_slice", summary: "Stored evidence", canResume: true };
  attachStore(handler, checkpoint);
  handler.context.assistantConversations.setRecoveryPending("display-old", true);
  await handler.resume();
  assert.equal(handler.context.assistantConversations.activeConversation.recoveryPending, true);
  assert.equal(handler.context.assistantConversations.activeConversation.continuation.summary, checkpoint.summary);
  assert.equal(handler.context.assistantRequestRef.current, false);
});

test("failed resume requires a separate state check and explicit second click before retrying its checkpoint", async () => {
  let resumes = 0, checks = 0;
  const checkpoint = { reason: "investigation_failed", summary: "Saved task and evidence", canResume: true };
  const handler = loadAppHandler(() => assert.fail("Resume must not interpret another prompt"), [], [], {}, {
    resumeAssistantConversation: async () => { if (++resumes === 1) throw new Error("response lost"); return reply(); },
    getAssistantConversationState: async (conversationId, _settings, afterCursor) => { if (afterCursor === undefined) checks += 1; return { conversationId, status: "paused", revision: 4, continuation: checkpoint }; },
  });
  attachStore(handler, checkpoint);
  await handler.resume();
  assert.equal(resumes, 1);
  assert.equal(handler.context.assistantConversations.activeConversation.recoveryPending, true);
  await handler.resume();
  assert.equal(checks, 1);
  assert.equal(resumes, 1, "Status checking must not resend resume");
  assert.equal(handler.context.assistantConversations.activeConversation.recoveryPending, false);
  assert.equal(handler.context.assistantConversations.activeConversation.continuation.summary, checkpoint.summary);
  await handler.resume();
  assert.equal(resumes, 2);
  assert.equal(handler.requests.length, 0);
});

for (const status of ["idle", "running", "unavailable"]) {
  test(`recovery state ${status} never claims success or replays a request`, async () => {
    const handler = loadAppHandler(() => assert.fail("State check cannot execute"), [], [], {}, {
      getAssistantConversationState: async (conversationId) => ({ conversationId, status, revision: status === "unavailable" ? null : 3, continuation: null }),
      resumeAssistantConversation: () => assert.fail("State check cannot resume"),
    });
    attachStore(handler);
    handler.context.assistantConversations.setRecoveryPending("display-old", true);
    await handler.resume();
    assert.equal(handler.states.at(-1).status, "inconclusive");
    const current = handler.context.assistantConversations.activeConversation;
    assert.equal(current.recoveryPending, status === "running");
    assert.equal(current.unavailable, status === "unavailable");
    assert.equal(current.continuation, null);
    assert.equal(handler.requests.length, 0);
  });
}

test("deleting a backend-expired conversation removes its display cache after the idempotent reply", async () => {
  const deleted = [];
  const handler = loadAppHandler(reply(), [], [], {}, {
    assistantExecution: { status: "idle" }, resetAssistantConversationUi: () => {},
    deleteAssistantConversation: async (conversationId) => { deleted.push(conversationId); return { conversationId, stopping: false }; },
  });
  const read = attachStore(handler);
  handler.context.assistantConversations.markUnavailable("display-old");
  await handler.remove("display-old");
  assert.deepEqual(deleted, ["fixture-conversation"]);
  assert.equal(read().conversations.length, 0);
});
