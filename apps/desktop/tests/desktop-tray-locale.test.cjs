const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function load(native = false) {
  const filename = path.resolve(__dirname, "../src/desktop-tray-locale.ts");
  const exports = {}, calls = [], errors = [];
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    console: { error: (...args) => errors.push(args) },
    require: () => ({
      isTauri: () => native,
      invoke: async (command, args) => calls.push({ command, locale: args.locale })
    })
  }, { filename });
  return { ...exports, calls, errors };
}

const settle = () => new Promise((resolve) => setImmediate(resolve));

test("native tray locale requests stay serial and coalesce rapid changes to the newest preference", async () => {
  const { createTrayLocaleSync } = load();
  const calls = [], completions = [];
  const sync = createTrayLocaleSync((locale) => {
    calls.push(locale);
    return new Promise((resolve) => completions.push(resolve));
  });
  sync("zh-CN");
  sync("en-US");
  sync("zh-CN");
  sync("en-US");
  assert.deepEqual(calls, ["zh-CN"]);
  completions.shift()();
  await settle();
  assert.deepEqual(calls, ["zh-CN", "en-US"]);
  completions.shift()();
  await settle();
  sync("zh-CN");
  assert.deepEqual(calls, ["zh-CN", "en-US", "zh-CN"]);
  completions.shift()();
  await settle();
});

test("a failed native update is reported and does not block the newest preference", async () => {
  const { createTrayLocaleSync, errors } = load();
  const calls = [];
  let rejectFirst;
  const sync = createTrayLocaleSync((locale) => {
    calls.push(locale);
    return calls.length === 1 ? new Promise((_, reject) => { rejectFirst = reject; }) : Promise.resolve();
  });
  sync("zh-CN");
  sync("en-US");
  rejectFirst(new Error("Native menu unavailable"));
  await settle();
  assert.deepEqual(calls, ["zh-CN", "en-US"]);
  assert.equal(errors.length, 1);
});

test("browser views skip native calls while desktop views send the selected locale", async () => {
  const browser = load(false);
  browser.syncDesktopTrayLocale("en-US");
  assert.deepEqual(browser.calls, []);
  const desktop = load(true);
  desktop.syncDesktopTrayLocale("en-US");
  await settle();
  assert.deepEqual(desktop.calls, [{ command: "set_tray_locale", locale: "en-US" }]);
});
