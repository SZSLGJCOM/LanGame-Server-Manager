const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = function(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { droppedFilePaths, modDropEffect, preventUnhandledDropNavigation } = require("../src/desktop-file-drop.ts");

test("Mod drop effects negotiate copy or reference-only link without moving the external source", () => {
  const protectedTransfer = (effectAllowed, types, items = []) => ({ effectAllowed, types, files: [], items,
    getData() { throw new Error("Protected dragover must not inspect content"); } });
  for (const mask of ["copy", "copyLink", "copyMove", "all", "uninitialized"]) {
    assert.equal(modDropEffect(protectedTransfer(mask, ["text/uri-list"]), true), "copy");
    assert.equal(modDropEffect(protectedTransfer(mask, ["Files"]), false), "copy");
  }
  for (const mask of ["link", "linkMove"]) {
    for (const type of ["text/plain", "text/uri-list", "text/html"])
      assert.equal(modDropEffect(protectedTransfer(mask, [type]), true), "link");
    assert.equal(modDropEffect(protectedTransfer(mask, ["text/uri-list"]), false), "none");
    assert.equal(modDropEffect(protectedTransfer(mask, ["Files", "text/uri-list"]), true), "link");
    assert.equal(modDropEffect(protectedTransfer(mask, ["text/uri-list"], [{ kind: "file" }]), true), "link");
    assert.equal(modDropEffect(protectedTransfer(mask, ["Files"]), true), "none");
    assert.equal(modDropEffect(protectedTransfer(mask, ["Files", "text/uri-list"]), false), "none");
  }
  for (const mask of ["none", "move", "unexpected"]) {
    assert.equal(modDropEffect(protectedTransfer(mask, ["text/plain"]), true), "none");
    assert.equal(modDropEffect(protectedTransfer(mask, ["Files"]), true), "none");
  }
  assert.equal(modDropEffect(protectedTransfer("copy", ["application/x-unsupported"]), true), "none");
  assert.equal(modDropEffect(protectedTransfer("copy", []), true), "none");
});

function host(reply, marker = true) {
  const listeners = new Set();
  const bridge = {
    addEventListener: (_, fn) => listeners.add(fn),
    removeEventListener: (_, fn) => listeners.delete(fn),
    postMessageWithAdditionalObjects() { throw new Error("Top-level Wry IPC must not receive dropped objects"); }
  };
  const sender = { postMessageWithAdditionalObjects(message, objects) { reply(message, objects, (data) => {
      for (const listener of listeners) listener({ data });
    }); } };
  global.window = { chrome: { webview: bridge }, location: { href: "http://127.0.0.1:43170/app" } };
  global.document = {
    createElement() {
      const events = new Map();
      return { isConnected: false, name: "", src: "", hidden: false,
        setAttribute() {}, addEventListener: (type, fn) => events.set(type, fn),
        removeEventListener: (type) => events.delete(type), remove() { this.isConnected = false; },
        contentWindow: { chrome: { webview: sender }, location: { href: "http://127.0.0.1:43170/lgsm-file-drop.html" },
          document: { querySelector: () => marker ? { getAttribute: () => "1" } : null } },
        loaded: () => events.get("load")?.(),
      };
    },
    body: { appendChild(frame) { frame.isConnected = true; queueMicrotask(frame.loaded); } },
  };
  return listeners;
}
function transfer(files, handles) {
  return { files, items: handles ? handles.map((handle) => ({ kind: "file", getAsFile: () => null,
    getAsFileSystemHandle: () => Promise.resolve(handle) })) : [] };
}
const signal = () => new AbortController().signal;
async function settle() { for (let pass = 0; pass < 12; pass++) await Promise.resolve(); }

test("text drops need no native bridge and file paths are never inferred from text", async () => {
  global.window = {};
  assert.deepEqual(await droppedFilePaths(transfer([]), signal()), []);
  await assert.rejects(droppedFilePaths(transfer([{ name: "mod.zip" }]), signal()), { code: "unavailable" });
});
test("native file and directory objects preserve the top-level selection", async () => {
  const file = { name: "mod.zip" }, directory = { kind: "directory", name: "Mod" };
  const listeners = host((message, objects, emit) => {
    assert.equal(message.type, "lgsm:resolve-dropped-files");
    assert.deepEqual(objects, [file, directory]);
    emit({ type: "lgsm:dropped-files", requestId: "unrelated", paths: ["wrong"] });
    emit({ type: "lgsm:dropped-files", requestId: message.requestId, paths: ["D:\\drop\\mod.zip", "D:\\drop\\Mod"] });
  });
  assert.deepEqual(await droppedFilePaths(transfer([], [file, directory]), signal()), ["D:\\drop\\mod.zip", "D:\\drop\\Mod"]);
  assert.equal(listeners.size, 0);
});

test("an available native File posts without waiting for an unresolved handle", async () => {
  const file = { name: "mod.zip" }, directoryFile = { name: "Mod", type: "" };
  let handleRequests = 0, posts = 0;
  const listeners = host((message, objects, emit) => {
    posts++;
    assert.deepEqual(objects, [file, directoryFile]);
    emit({ type: "lgsm:dropped-files", requestId: message.requestId, paths: ["D:\\drop\\mod.zip", "D:\\drop\\Mod"] });
  });
  const selected = { files: [file, directoryFile], items: [file, directoryFile].map((entry) => ({
    kind: "file", getAsFile: () => entry,
    getAsFileSystemHandle: () => { handleRequests++; return new Promise(() => {}); },
  })) };
  assert.deepEqual(await droppedFilePaths(selected, signal()), ["D:\\drop\\mod.zip", "D:\\drop\\Mod"]);
  assert.equal(handleRequests, 0);
  assert.equal(posts, 1);
  assert.equal(listeners.size, 0);
});

test("a missing File and null handle are rejected without dropping the selection", async () => {
  let posts = 0;
  const listeners = host(() => posts++);
  await assert.rejects(droppedFilePaths(transfer([], [null]), signal()), { code: "unavailable", stage: "handles" });
  assert.equal(posts, 0);
  assert.equal(listeners.size, 0);
});

test("a missing static frame marker rejects SPA fallback before forwarding files", async () => {
  let posts = 0;
  const listeners = host(() => posts++, false);
  await assert.rejects(droppedFilePaths(transfer([{ name: "mod.zip" }]), signal()), { code: "unavailable", stage: "frame" });
  assert.equal(posts, 0);
  assert.equal(listeners.size, 0);
});
test("a failed native path result is not partially installed", async () => {
  const listeners = host((message, _, emit) => emit({ type: "lgsm:dropped-files", requestId: message.requestId,
    paths: ["D:\\drop\\valid.zip"], error: "unsupported_object" }));
  await assert.rejects(droppedFilePaths(transfer([{ name: "mod.zip" }]), signal()), { code: "unavailable" });
  assert.equal(listeners.size, 0);
});
test("leaving the target cancels a pending reply and removes its listener", async () => {
  let respond;
  const listeners = host((message, _, emit) => { respond = () => emit({ type: "lgsm:dropped-files",
    requestId: message.requestId, paths: ["D:\\drop\\late.zip"] }); });
  const controller = new AbortController();
  const pending = droppedFilePaths(transfer([{ name: "late.zip" }]), controller.signal);
  await settle();
  controller.abort();
  await assert.rejects(pending, { name: "AbortError" });
  respond?.();
  assert.equal(listeners.size, 0);
});
test("oversized drops fail before forwarding objects", async () => {
  let calls = 0;
  host(() => calls++);
  await assert.rejects(droppedFilePaths(transfer(Array(257).fill({ name: "mod.zip" })), signal()), { code: "too_many" });
  assert.equal(calls, 0);
});

test("a handle resolution timeout is distinct from a native reply timeout", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  let posts = 0;
  const listeners = host(() => posts++);
  let release;
  const pendingHandle = new Promise((resolve) => { release = resolve; });
  const unresolved = { files: [], items: [{ kind: "file", getAsFile: () => null,
    getAsFileSystemHandle: () => pendingHandle }] };
  const pending = droppedFilePaths(unresolved, signal());
  const rejected = assert.rejects(pending, { code: "unavailable", stage: "handles", message: "unavailable [handles]" });
  context.mock.timers.tick(5000);
  await rejected;
  release({ kind: "directory" });
  await settle();
  assert.equal(posts, 0, "A timed-out handle must never forward a late request");
  assert.equal(listeners.size, 0);
});

test("an unanswered posted request reports its native reply stage", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  let posts = 0;
  const listeners = host(() => posts++);
  const pending = droppedFilePaths(transfer([{ name: "mod.zip" }]), signal());
  const rejected = assert.rejects(pending, { code: "unavailable", stage: "native_reply", message: "unavailable [native_reply]" });
  await settle();
  assert.equal(posts, 1);
  context.mock.timers.tick(5000);
  await rejected;
  assert.equal(listeners.size, 0);
});
test("unhandled drops cannot navigate while accepted drop targets retain copy feedback", () => {
  const listeners = new Map();
  global.window = { addEventListener: (type, fn) => listeners.set(type, fn), removeEventListener: (type) => listeners.delete(type) };
  const cleanup = preventUnhandledDropNavigation();
  const accepted = { defaultPrevented: true, dataTransfer: { dropEffect: "copy" }, preventDefault() { throw Error("Already accepted"); } };
  listeners.get("dragover")(accepted);
  assert.equal(accepted.dataTransfer.dropEffect, "copy");
  const outside = { defaultPrevented: false, dataTransfer: { dropEffect: "copy" }, preventDefault() { this.defaultPrevented = true; } };
  listeners.get("drop")(outside);
  assert.equal(outside.defaultPrevented, true);
  assert.equal(outside.dataTransfer.dropEffect, "none");
  cleanup(); assert.equal(listeners.size, 0);
});
test("the desktop leaves DOM drag/drop enabled instead of replacing the WebView2 target", () => {
  const config = JSON.parse(fs.readFileSync(path.join(__dirname, "../src-tauri/tauri.conf.json"), "utf8"));
  assert.equal(config.app.windows.find((window) => window.label === "main").dragDropEnabled, false);
  const resource = fs.readFileSync(path.join(__dirname, "../public/lgsm-file-drop.html"), "utf8");
  assert.match(resource, /<meta name="lgsm-file-drop" content="1">/);
  assert.doesNotMatch(resource, /<script\b/i);
});
