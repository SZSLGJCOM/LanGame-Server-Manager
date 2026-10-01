const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
const uiStateSource = fs.readFileSync(
  path.join(desktopRoot, "src", "hooks", "useDesktopUiState.ts"),
  "utf8"
);
const routerSource = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "AppViewRouter.tsx"),
  "utf8"
);
const appSource = fs.readFileSync(path.join(desktopRoot, "src", "App.tsx"), "utf8");
const shellSource = fs.readFileSync(
  path.join(desktopRoot, "src", "components", "AppShell.tsx"),
  "utf8"
);
const headerSource = fs.readFileSync(
  path.join(desktopRoot, "src", "components", "AppHeader.tsx"),
  "utf8"
);

test("top-level navigation selects the requested view synchronously", () => {
  const state = [];
  let cursor = 0;
  const exports = {};
  vm.runInNewContext(transpileTypeScript(uiStateSource, "useDesktopUiState.ts"), {
    exports,
    require: (name) => {
      assert.equal(name, "react");
      return { useState(initial) {
        const index = cursor++;
        if (!(index in state)) state[index] = initial;
        return [state[index], (next) => { state[index] = next; }];
      } };
    }
  });
  const render = () => {
    cursor = 0;
    return exports.useDesktopUiState({ onSelectInstance() {}, onSelectModule() {} });
  };
  assert.equal(render().activeView, "system");
  render().handleNavSelect("servers");
  assert.equal(render().activeView, "servers");
  render().handleNavSelect("library");
  assert.equal(render().activeView, "library");
});

test("route imports remain deferred with an immediate local loading and retry surface", () => {
  assert.match(routerSource, /const serverWorkspaceModule = createDeferredModule\(\s*\(\) =>/);
  assert.match(routerSource, /useDeferredModule\(serverWorkspaceModule, props.activeView === "servers"\)/);
  assert.match(routerSource, /return <ViewLoadingState error=\{selected.error\} canRetry=\{selected.canRetry\} onRetry=\{selected.retry\} \/>/);
  assert.doesNotMatch(routerSource, /\b(?:Suspense|lazy)\b/);
});

test("a failed route stays local, exposes retry, and renders the recovered component", () => {
  const exports = {};
  let sourceCount = 0;
  let retries = 0;
  let current = { status: "loading", value: null, error: null, canRetry: true, retry: () => { retries++; } };
  const node = (type, props) => ({ type, props });
  const dependencies = {
    "react/jsx-runtime": { jsx: node, jsxs: node },
    "../deferred-module": { createDeferredModule: () => sourceCount++ },
    "../hooks/useDeferredModule": { useDeferredModule: (source, enabled) => {
      if (enabled) assert.equal(source, 1, "only the selected server route may load");
      return enabled ? current : { status: "idle", value: null, error: null, retry() {} };
    } },
    "../i18n": { useI18n: () => ({ t: (_key, _values, fallback) => fallback }) }
  };
  vm.runInNewContext(transpileTypeScript(routerSource, "AppViewRouter.tsx"), {
    exports,
    require: (name) => {
      assert.ok(Object.hasOwn(dependencies, name), `Unexpected dependency: ${name}`);
      return dependencies[name];
    }
  });
  const render = () => {
    const element = exports.AppViewRouter({ activeView: "servers" });
    return element.type(element.props);
  };
  assert.equal(render().props.role, "status");
  current = { ...current, status: "error", error: new Error("Chunk unavailable") };
  const failed = render();
  assert.equal(failed.props.role, "alert");
  const retry = failed.props.children.find((element) => element?.type === "button");
  retry.props.onClick();
  assert.equal(retries, 1);
  current = { ...current, canRetry: false };
  const exhausted = render();
  assert.equal(exhausted.props.role, "alert");
  assert.ok(!exhausted.props.children.some((element) => element?.type === "button"));
  assert.match(exhausted.props.children[0].props.children, /Your current work is preserved/);
  const recovered = () => "server content";
  current = { ...current, status: "ready", error: null, value: recovered };
  assert.equal(render(), "server content");
});

test("the shell contract contains no retired sidebar, context, or header-search props", () => {
  const retiredProps = [
    "activeContextLabel",
    "sidebarCollapsed",
    "onExitHint",
    "onToggleSidebar",
    "searchPlaceholder",
    "searchValue",
    "showSearch",
    "onSearchChange"
  ];
  for (const prop of retiredProps) {
    assert.ok(!shellSource.includes(prop), `AppShell still exposes retired prop ${prop}`);
    assert.ok(!headerSource.includes(prop), `AppHeader still exposes retired prop ${prop}`);
  }

  assert.match(appSource, /<AppViewRouter[\s\S]*onSearchChange=\{handleSearchChange\}/);
  assert.match(uiStateSource, /function handleSearchChange\(value: string\)/);
});
