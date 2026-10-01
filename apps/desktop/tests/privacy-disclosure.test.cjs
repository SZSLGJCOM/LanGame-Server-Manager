const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function load(name) {
  const filename = path.join(__dirname, "../src", name);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { exports, URL });
  return exports;
}
const { describeAiDataRecipient } = load("ai-data-recipient.ts");
const { readPrivacyNoticeSections } = load("privacy-notice-document.ts");

test("recipient display strips URL credentials, private paths, query and fragment", () => {
  const target = describeAiDataRecipient("https://user:fixture@MODEL.example:8443/private-project/v1?key=private-token#private-fragment");
  assert.equal(target.origin, "https://model.example:8443");
  assert.equal(target.encrypted, true);
  assert.equal(target.loopback, false);
  assert.doesNotMatch(JSON.stringify(target), /private-|user:fixture/);
});

test("invalid and unsupported endpoints never display the raw value or imply a known operator", () => {
  for (const value of ["", "secret-token", "/v1?key=secret", "javascript:secret", "file:///secret", "ftp://localhost/secret", "https://", "https://secret@", "http://[invalid]"]) {
    assert.equal(describeAiDataRecipient(value), null);
  }
});

test("local classification follows only the parsed loopback host, independently from transport", () => {
  for (const value of ["http://127.0.0.1:11434/v1", "http://127.20.30.40/v1", "http://[::1]:11434", "http://localhost", "http://LOCALHOST."]) {
    assert.equal(describeAiDataRecipient(value).loopback, true, value);
    assert.equal(describeAiDataRecipient(value).encrypted, false, value);
  }
  for (const value of ["http://192.168.1.2:11434", "http://localhost.example/v1", "https://models.example/ollama", "http://0.0.0.0", "https://127.0.0.1.example"]) {
    assert.equal(describeAiDataRecipient(value).loopback, false, value);
  }
  assert.equal(describeAiDataRecipient("https://localhost").encrypted, true);
});

test("both offline notices include every canonical section and paragraph in their own language", () => {
  const document = fs.readFileSync(path.join(__dirname, "../../../PRIVACY.md"), "utf8");
  for (const language of ["English", "简体中文"]) {
    const canonical = document.split(`## ${language}`)[1].split(/\r?\n## /)[0].trim();
    const sections = readPrivacyNoticeSections(document, language);
    assert.ok(sections.length >= 8);
    assert.equal(sections.map(({ title, paragraphs }) => `### ${title}\n\n${paragraphs.join("\n\n")}`).join("\n\n"), canonical.replace(/\r\n/g, "\n"));
    assert.ok(sections.every(({ title, paragraphs }) => title && paragraphs.length > 0));
  }
});

test("plain text parser preserves untrusted-looking text without evaluating it", () => {
  const sections = readPrivacyNoticeSections("## English\n\n### Heading\n\n<script>secret()</script>\n\n## 简体中文\n\n### 标题\n\n正文", "English");
  assert.equal(sections.length, 1);
  assert.equal(sections[0].paragraphs[0], "<script>secret()</script>");
});
