const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
const { selectActiveDstRegenerationJob, selectFinishedDstRegenerationJob, dstRegenerationOutcomeKey } = require("../src/dst-regeneration-job.ts");
const { DstRegenerationActivity, DstRegenerationOutcome } = require("../src/components/DstRegenerationActivity.tsx");
const job = (changes = {}) => ({ id: "dst-world-regeneration:a", label: "Forest", kind: "RegenerateWorld", status: "Running",
  cancellable: false, cancel_requested: false, detail: "stopping", ...changes });
const plain = element => renderToStaticMarkup(element).replace(/<[^>]*>/g, "");

test("every backend step is shown in the activity bar", () => {
  for (const [detail, label] of [["stopping", "正在停服"], ["generating", "正在生成新世界"]]) {
    const active = selectActiveDstRegenerationJob([job({ kind: "DownloadGame" }), job({ detail })]);
    assert.equal(active?.detail, detail);
    assert.match(plain(React.createElement(DstRegenerationActivity, { job: active, locale: "zh-CN" })), new RegExp(`Forest ${label}`));
  }
  assert.equal(selectActiveDstRegenerationJob([job({ detail: "awaiting_confirmation" })]), null);
  assert.equal(selectActiveDstRegenerationJob([job({ status: "Completed", detail: "done" })]), null);
});

test("only outcomes watched this session are announced, until dismissed or run again", () => {
  const seen = new Set(), dismissed = new Set();
  assert.equal(selectFinishedDstRegenerationJob([job({ status: "Completed", detail: "done" })], seen, dismissed), null, "an earlier session's outcome stays quiet");
  selectFinishedDstRegenerationJob([job()], seen, dismissed);
  const done = job({ status: "Completed", detail: "done" });
  assert.equal(selectFinishedDstRegenerationJob([done], seen, dismissed), done);
  const doneHtml = renderToStaticMarkup(React.createElement(DstRegenerationOutcome, { job: done, locale: "zh-CN", dismissLabel: "关闭", onDismiss() {} }));
  assert.match(doneHtml, /新世界已生成/);
  assert.match(doneHtml, /is-success/);
  dismissed.add(dstRegenerationOutcomeKey(done));
  assert.equal(selectFinishedDstRegenerationJob([done], seen, dismissed), null);
  selectFinishedDstRegenerationJob([job({ detail: "generating" })], seen, dismissed);
  const failed = job({ status: "Failed", detail: "failed" });
  assert.equal(selectFinishedDstRegenerationJob([failed], seen, dismissed), failed, "a new run announces again");
  assert.match(renderToStaticMarkup(React.createElement(DstRegenerationOutcome, { job: failed, locale: "zh-CN", dismissLabel: "关闭", onDismiss() {} })), /is-error.*世界重新生成失败/);
});
