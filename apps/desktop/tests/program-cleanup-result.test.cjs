const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { programCleanupDetails, emptyProgramCleanup } = require("./helpers/program-cleanup-fixture.cjs");

function catalog(filename, exportName) {
  const absolute = path.join(__dirname, "../src", filename);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(absolute, "utf8"), absolute), { exports }, { filename: absolute });
  return exports[exportName];
}
for (const [locale, messages] of [
  ["en-US", catalog("i18n-messages-en-extra.ts", "EN_US_EXTRA_MESSAGES")],
  ["zh-CN", catalog("i18n-messages-zh-extra.ts", "ZH_CN_EXTRA_MESSAGES")]
]) {
  const t = (key, params, fallback) => String(messages[key] ?? fallback ?? key)
    .replace(/\{([^}]+)\}/g, (match, name) => String(params?.[name] ?? match));
  test(`${locale}: ordinary program cleanup needs no additional notice`, () => {
    const result = emptyProgramCleanup(); result.removed_install_roots = ["fixture/extra"];
    assert.equal(programCleanupDetails(result, t), "");
  });
  test(`${locale}: retained programs include paths and localized reasons without hiding unfamiliar failures`, () => {
    const reasons = ["in_use", "archive_dependency", "unverified_package", "unsafe_path", "cleanup_failed"];
    const result = { removed_install_roots: [], preserved_data_paths: ["fixture/world", "fixture/world"],
      retained_installs: reasons.map((reason) => ({ install_root: `fixture/${reason}`, reason })) };
    result.retained_installs.push({ install_root: "fixture/recovery", reason: "IO_FAILURE_SENTINEL" });
    const text = programCleanupDetails(result, t);
    assert.equal(text.split("fixture/world").length - 1, 1);
    for (const reason of reasons) {
      assert.ok(text.includes(`fixture/${reason}\n${messages[`programCleanup.reason.${reason}`]}`));
    }
    assert.match(text, /fixture\/recovery\nIO_FAILURE_SENTINEL/);
    assert.doesNotMatch(text, /programCleanup\./);
  });
}
