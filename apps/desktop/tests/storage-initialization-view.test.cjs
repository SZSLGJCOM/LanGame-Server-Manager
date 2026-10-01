const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const viewPath = path.resolve(__dirname, "../src/components/StorageInitializationView.tsx");
let catalog;

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    if (filename === viewPath) {
      const load = module.require.bind(module);
      module.require = (request) => request === "../i18n"
        ? { useI18n: () => ({ t: (key) => catalog[key] ?? key }) }
        : load(request);
    }
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}

const { StorageInitializationView } = require(viewPath);
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_CORE_MESSAGES } = require("../src/i18n-messages-zh-core.ts");
const failure = {
  status: "failed",
  attempt: 0,
  error: "database migration history is incompatible: migration 1 does not match the supported successful history\nThe existing database was not converted.",
  databasePath: "C:/data/db/lgs.db",
  logPath: "C:/data/logs/desktop-app.log",
  logDirectory: "C:/data/logs"
};

function render(state, onRetry = () => {}) {
  return renderToStaticMarkup(React.createElement(StorageInitializationView, { state, onRetry }));
}

for (const [locale, messages] of [["en-US", EN_US_MESSAGES], ["zh-CN", ZH_CN_CORE_MESSAGES]]) {
  test(`${locale} failure renders a visible cause preview and native expandable full diagnostics`, () => {
    catalog = messages;
    const html = render(failure);
    const summary = html.match(/<summary>([\s\S]*?)<\/summary>/)?.[1];
    const fullError = html.match(/<pre[^>]*>([\s\S]*?)<\/pre>/)?.[1];

    assert.ok(html.includes(messages["storage.initialization.failedBody"]));
    assert.match(html, /role="alert"/);
    assert.match(html, /<details class="storage-initialization-details">/);
    assert.ok(summary?.includes(messages["storage.initialization.errorDetails"]));
    assert.ok(summary?.includes(failure.error), "the cause is visible before expanding details");
    assert.equal(fullError, failure.error, "expanding details exposes the complete original error");
    assert.ok(html.includes(messages["storage.initialization.logPath"]));
    assert.ok(html.includes(failure.logPath));
    assert.ok(!summary.includes(failure.logPath), "the log path remains inside collapsed details");
    assert.ok(!html.includes(failure.databasePath), "unrelated storage metadata is not exposed");
    assert.match(html, /<button type="button" class="storage-initialization-retry-button">/);
    assert.ok(html.includes(messages["storage.initialization.retry"]));
    assert.doesNotMatch(html, /storage\.initialization\./);
  });
}

test("failure without storage metadata keeps the error and escapes untrusted diagnostic text", () => {
  catalog = EN_US_MESSAGES;
  const html = render({
    ...failure,
    error: 'Failed to read <script>alert("failure")</script> & database',
    databasePath: null,
    logPath: null,
    logDirectory: null
  });

  assert.match(html, /Failed to read &lt;script&gt;alert\(&quot;failure&quot;\)&lt;\/script&gt; &amp; database/);
  assert.doesNotMatch(html, /<script>|storage-initialization-log/);
});

test("loading and ready states do not expose stale diagnostics or a retry action", () => {
  catalog = EN_US_MESSAGES;
  for (const attempt of [0, 1]) {
    const html = render({ status: "pending", attempt });
    assert.match(html, /role="status"/);
    assert.match(html, /aria-busy="true"/);
    assert.ok(html.includes(EN_US_MESSAGES[attempt > 0
      ? "storage.initialization.retryingTitle"
      : "storage.initialization.loadingTitle"]));
    assert.doesNotMatch(html, /<details|<button|migration history/);
  }
  assert.equal(render({ status: "ready", attempt: 1 }), "");
});

test("the failure retry action delegates exactly once to the existing initialization handler", () => {
  catalog = EN_US_MESSAGES;
  let retries = 0;
  const tree = StorageInitializationView({ state: failure, onRetry: () => { retries += 1; } });
  const button = React.Children.toArray(tree.props.children.props.children)
    .find((child) => child.type === "button");

  assert.ok(button);
  assert.equal(button.props.disabled, undefined);
  button.props.onClick();
  assert.equal(retries, 1);
});
