const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
const { inspectWorkshopManifest, parseWorkshopManifest } = require("../src/views/servers/workshop-manifest.ts");
const filename = path.resolve(__dirname, "../src/views/servers/WorkshopManifestPanel.tsx");
const source = fs.readFileSync(filename, "utf8");
const functions = new Map();
let contextEffect;
visitSyntax(parseSource(source, filename), node => {
  if (node.type === "FunctionDeclaration" && ["check", "apply"].includes(node.identifier?.value)) {
    functions.set(node.identifier.value, sourceText(source, node));
  }
  if (node.type === "CallExpression" && node.callee.value === "useEffect" &&
      node.arguments[1]?.expression?.elements?.some(item => item?.expression?.value === "contextKey")) {
    assert.equal(contextEffect, undefined);
    contextEffect = sourceText(source, node.arguments[0].expression);
  }
});
assert.equal(functions.size, 2);
assert.ok(contextEffect);
const script = transpileTypeScript(`${[...functions.values()].join("\n")}\nexports.check = check; exports.apply = apply; exports.changeContext = ${contextEffect};`, filename);

const item = (locale) => ({ id: "1000001", title: `${locale} title`, consumer_app_id: 322330,
  status: "resolved", item_kind: "item", children: [], child_count: 0 });

function harness(options = {}) {
  const generation = { current: 0 }, currentContext = { current: "" }, mounted = { current: true };
  const state = { review: null, checking: false, applying: false, error: null, message: null,
    text: "1000001", calls: [], applied: [] };
  function render(locale, commit = true) {
    const contextKey = JSON.stringify(["fixture-instance", 322330, locale]);
    currentContext.current = contextKey;
    const exports = {};
    vm.runInNewContext(script, {
      exports, locale, contextKey, currentContext, generation, mounted, inspectWorkshopManifest,
      parsed: parseWorkshopManifest(state.text), review: state.review, applying: state.applying,
      props: { instanceId: "fixture-instance", appId: 322330, disabled: false, readOnly: Boolean(options.readOnly),
        onApply: async (review, enable) => {
          state.applied.push({ review, enable });
          return options.apply ? options.apply(review, enable) : true;
        } },
      lookupSteamWorkshopItems: async (ids, requestedLocale) => {
        state.calls.push({ ids: [...ids], locale: requestedLocale });
        return options.lookup ? options.lookup(ids, requestedLocale) : [item(requestedLocale)];
      },
      readSteamWorkshopInstallationStatus: async () => ({ consumer_app_id: 322330, items: [], searched_roots: [] }),
      describeError: error => error.message,
      t: key => `${locale}:${key}`,
      setReviewContext: value => { state.context = value; },
      setReview: value => { state.review = value; },
      setChecking: value => { state.checking = value; },
      setApplying: value => { state.applying = value; },
      setError: value => { state.error = value; },
      setMessage: value => { state.message = value; }
    }, { filename });
    if (commit && state.context !== contextKey) exports.changeContext();
    return exports;
  }
  return { state, render };
}

test("manifest review pins lookup locale and ignores the old response even before the context effect commits", async () => {
  const pending = Promise.withResolvers();
  const h = harness({ lookup: (_ids, locale) => locale === "en-US" ? pending.promise : [item(locale)] });
  const english = h.render("en-US");
  const oldReview = english.check();
  assert.equal(h.state.checking, true);
  const chinese = h.render("zh-CN", false);
  pending.resolve([item("en-US")]);
  await oldReview;
  assert.equal(h.state.review, null);
  assert.equal(h.state.error, null);
  chinese.changeContext();
  await chinese.check();
  assert.deepEqual(h.state.calls.map(call => call.locale), ["en-US", "zh-CN"]);
  assert.equal(h.state.review.items["1000001"].title, "zh-CN title");
  assert.equal(h.state.checking, false);
  assert.equal(h.state.text, "1000001");
});

test("switching language clears review errors but never releases an outstanding installation", async () => {
  const installation = Promise.withResolvers();
  const h = harness({ apply: () => installation.promise });
  await h.render("en-US").check();
  const reviewed = h.state.review;
  const applying = h.render("en-US").apply(true);
  assert.equal(h.state.applying, true);
  h.state.error = "old language feedback";
  const chinese = h.render("zh-CN");
  assert.equal(h.state.review, null);
  assert.equal(h.state.error, null);
  assert.equal(h.state.message, null);
  assert.equal(h.state.applying, true, "only the operation's finally can release its ownership");
  assert.equal(h.state.text, "1000001");
  installation.resolve(true);
  await applying;
  assert.equal(h.state.applying, false);
  assert.equal(h.state.message, null);
  assert.equal(h.state.calls.length, 1, "completion cannot start another old-language review");
  assert.strictEqual(h.state.applied[0].review, reviewed);
  await chinese.check();
  assert.equal(h.state.calls.at(-1).locale, "zh-CN");
  assert.equal(h.state.review.items["1000001"].title, "zh-CN title");
});

test("late review failures from a prior language cannot overwrite the new review", async () => {
  const pending = Promise.withResolvers();
  const h = harness({ lookup: (_ids, locale) => locale === "en-US" ? pending.promise : [item(locale)] });
  const oldReview = h.render("en-US").check();
  await h.render("zh-CN").check();
  pending.reject(new Error("English network failure"));
  await oldReview;
  assert.equal(h.state.review.items["1000001"].title, "zh-CN title");
  assert.equal(h.state.error, null);
  assert.equal(h.state.checking, false);
});

test("archived manifest cannot inspect the original instance or apply a previously reviewed plan", async () => {
  const options = {};
  const h = harness(options);
  await h.render("en-US").check();
  assert.equal(h.state.calls.length, 1);
  options.readOnly = true;
  await h.render("en-US").check();
  await h.render("en-US").apply(true);
  assert.equal(h.state.calls.length, 1);
  assert.deepEqual(h.state.applied, []);
  assert.equal(h.state.applying, false);
});