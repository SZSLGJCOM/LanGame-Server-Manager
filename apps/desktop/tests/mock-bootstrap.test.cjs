const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function loadBootstrap() {
  const filename = path.join(__dirname, "../src/api-mock/bootstrap.ts");
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    require: (id) => {
      assert.equal(id, "./module-assets");
      return { buildMockModuleSummaries: () => [] };
    }
  }, { filename });

  return exports.mockBootstrap;
}

test("the initial snapshot counts Running instances without treating partial failures as healthy", () => {
  const { instances, snapshot } = loadBootstrap().state;
  const running = instances.filter((instance) => instance.status === "Running");
  const partialFailures = instances.filter((instance) => instance.status === "Error" && instance.active_process_count > 0);
  const terminalFailures = instances.filter((instance) => instance.status === "Error" && instance.active_process_count === 0);
  assert.ok(running.length > 1, "the preview must exercise more than one running instance");
  assert.ok(partialFailures.length > 0, "the preview must exercise failed instances with surviving processes");
  assert.ok(terminalFailures.length > 0, "fully stopped failures must not count as running");
  assert.equal(snapshot.running_instances, running.length);
  assert.ok(snapshot.running_instances <= instances.length);
});

test("mock summary updates retain failed processes without counting them as Running", () => {
  const mockBootstrap = loadBootstrap();
  const filename = path.join(__dirname, "../src/api-mock.ts");
  const source = fs.readFileSync(filename, "utf8");
  const declaration = parseSource(source, filename).statements.find(
    (node) => ts.isFunctionDeclaration(node) && node.name.text === "upsertMockSummary"
  );
  assert.ok(declaration);
  const upsert = vm.runInNewContext(
    transpileTypeScript(sourceText(source, declaration), filename) + "\nupsertMockSummary;",
    { mockBootstrap }, { filename }
  );
  const running = mockBootstrap.state.instances.filter((instance) => instance.status === "Running");
  const failed = { ...running[0], status: "Error", active_process_count: 1 };

  upsert(failed);
  assert.equal(mockBootstrap.state.snapshot.running_instances, running.length - 1);
  assert.equal(mockBootstrap.state.instances.find((instance) => instance.id === failed.id).active_process_count, 1);

  upsert({ ...failed, id: "additional-partial-failure" });
  assert.equal(mockBootstrap.state.snapshot.running_instances, running.length - 1);

  upsert({ ...running[0], id: "additional-running-instance" });
  assert.equal(mockBootstrap.state.snapshot.running_instances, running.length);
});
