const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { loadStorageManagementRequests } = require("./helpers/storage-management-requests.cjs");

function loadApi(tauri) {
  const filename = path.join(__dirname, "..", "src", "api.ts");
  const module = { exports: {} };
  const opened = [];
  const invoked = [];
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    module,
    exports: module.exports,
    URL,
    window: { open: (...args) => opened.push(args) },
    require: (name) => {
      if (name === "@tauri-apps/api/core") {
        return {
          isTauri: () => tauri,
          invoke: async (...args) => { invoked.push(args); }
        };
      }
      if (name === "./locale-preference") return { readPreferredLocale: () => "en-US" };
      if (name === "./storage-management-requests") return loadStorageManagementRequests({ readPreferredLocale: () => "en-US" });
      if (name === "./api-transport") return { invokeOrMock: async (...args) => { invoked.push(args); } };
      throw new Error(`Unexpected import: ${name}`);
    }
  }, { filename });
  return { openExternalUrl: module.exports.openExternalUrl, opened, invoked };
}

for (const tauri of [false, true]) {
  const runtime = tauri ? "desktop" : "browser";
  test(`${runtime} rejects unsafe or invalid external URLs before navigation`, async () => {
    const api = loadApi(tauri);
    for (const url of [
      "javascript:alert(1)", "JaVaScRiPt:alert(1)", "java\nscript:alert(1)",
      "data:text/html,untrusted", "file:///C:/fixture.txt", "cmd:fixture",
      "//example.com/path", "/relative", "https://", "", "   ",
      "https:example.com", "https://user:password@example.com", "https://user@example.com",
      "https://example.com\\file", "https://exam\nple.com", "https://example.com/\0payload",
      "\thttps://example.com", "https://[::1", "https://example.com:70000", "https:///example.com",
      `https://example.com/${"a".repeat(8192)}`
    ]) {
      await assert.rejects(() => api.openExternalUrl(url), /http\(s\)/i, url);
    }
    assert.equal(api.opened.length, 0);
    assert.equal(api.invoked.length, 0);
  });

  test(`${runtime} opens valid HTTP(S) links without opener access`, async () => {
    const api = loadApi(tauri);
    for (const [input, expected] of [
      ["  https://example.com/news?id=42#details  ", "https://example.com/news?id=42#details"],
      ["HTTP://example.com/help", "http://example.com/help"]
    ]) {
      await api.openExternalUrl(input);
      if (tauri) {
        const [command, args] = api.invoked.at(-1);
        assert.equal(command, "open_external_url");
        assert.equal(args.url, expected);
      } else {
        assert.deepEqual(api.opened.at(-1), [expected, "_blank", "noopener,noreferrer"]);
      }
    }
    assert.equal(tauri ? api.opened.length : api.invoked.length, 0);
  });
}
