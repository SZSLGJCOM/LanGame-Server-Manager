const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("@typescript/typescript6");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");
const filename = path.resolve(__dirname, "../src/hooks/useDesktopActions.ts");
const source = fs.readFileSync(filename, "utf8");

function action(name, context) {
  let declaration;
  visitSyntax(parseSource(source, filename), node => {
    if (ts.isFunctionDeclaration(node) && node.name?.text === name) { declaration = node; return false; }
  });
  assert.ok(declaration);
  return vm.runInNewContext(transpileTypeScript(sourceText(source, declaration), filename) + "; " + name, context);
}

for (const intent of ["Start", "Stop"]) {
  test(`a delayed ${intent.toLowerCase()} receipt preserves the instance chosen during navigation`, async () => {
    let selected = "seven-days";
    let complete;
    let invoked;
    const entered = new Promise(resolve => { invoked = resolve; });
    const mutation = new Promise(resolve => { complete = resolve; });
    const refreshTargets = [];
    const run = action(`handle${intent}Server`, {
      options: {
        setActivity() {}, markRuntimeRefreshed() {},
        setSelectedInstanceId(id) { selected = id; },
        async reloadBootstrap(preferred) {
          refreshTargets.push(preferred);
          if (preferred?.instanceId) selected = preferred.instanceId;
        }
      },
      settingsSaveCoordinator: { flush: async () => {} },
      startInstance() { invoked(); return mutation; },
      stopInstance() { invoked(); return mutation; },
      message: key => ({ key }),
      serverStartSuccessMessage: () => ({ key: "started" }),
      reportOperationRefreshFailure(error) { throw error; }
    });
    const pending = run("seven-days");
    await entered;
    selected = "new-asa";
    complete({});
    await pending;
    assert.equal(selected, "new-asa");
    assert.deepEqual(refreshTargets, [undefined], "Refresh must reconcile the current selection at response time");
  });
}
