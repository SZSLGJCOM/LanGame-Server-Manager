const assert = require("node:assert/strict");
const test = require("node:test");
const { isLanguageNeutralMessage } = require("../scripts/i18n-language-neutral.cjs");

test("format templates with only placeholders and punctuation are language neutral", () => {
  for (const value of ["{name}：{phase}", "（{value}）", "{done} / {total} · {percent}%", "{name} — {status}", "12:30", "{value}"]) {
    assert.equal(isLanguageNeutralMessage(value), true, value);
  }
});

test("English words and corrupted text remain non-neutral after placeholders are removed", () => {
  for (const value of ["{name}: Downloading", "Waiting for {name}", "{done} bytes / {total}", "{phase}�", "Please wait…"]) {
    assert.equal(isLanguageNeutralMessage(value), false, value);
  }
});
