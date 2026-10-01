const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(
  transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename
);
const { prepareDstWorldStart } = require("../src/views/servers/dst-world-start.ts");
const preview = (state = "new", caves = "new", settings = {}) => ({
  instance_id: "dst", settings_json: JSON.stringify(settings),
  shards: [{ shard: "Master", state, enabled: true }, { shard: "Caves", state: caves, enabled: true }]
});

for (const [master, caves] of [["new", "new"], ["existing", "existing"], ["existing", "new"]]) {
  test(`${master} surface / ${caves} caves start after persistence without another user decision`, async () => {
    const events = [];
    const saved = preview(master, caves, { master_world_size: "huge", caves_branching: "most" });
    const result = await prepareDstWorldStart("dst", {
      flush: async () => { events.push("save"); },
      preview: async () => { events.push("read"); return saved; }
    });
    assert.equal(result, saved, "the complete saved snapshot goes to the atomic backend start check");
    assert.deepEqual(events, ["save", "read"]);
  });
}

test("save failure prevents inspecting or launching the world", async () => {
  await assert.rejects(prepareDstWorldStart("dst", {
    flush: async () => { throw Error("save failed"); },
    preview: () => assert.fail("must not preview")
  }), /save failed/);
});

test("world inspection errors prevent startup", async () => {
  await assert.rejects(prepareDstWorldStart("dst", {
    flush: async () => {}, preview: async () => { throw Error("access denied"); }
  }), /access denied/);
});

test("unrecognized enabled save data still blocks generation", async () => {
  await assert.rejects(prepareDstWorldStart("dst", {
    flush: async () => {}, preview: async () => preview("unrecognized")
  }), (error) => error.code === "unrecognized");
});

test("disabled caves with unknown data do not block a valid surface", async () => {
  const value = preview("new", "unrecognized");
  value.shards[1].enabled = false;
  assert.equal(await prepareDstWorldStart("dst", {
    flush: async () => {}, preview: async () => value
  }), value);
});

test("a snapshot for another instance cannot authorize startup", async () => {
  await assert.rejects(prepareDstWorldStart("another", {
    flush: async () => {}, preview: async () => preview()
  }), (error) => error.code === "changed");
});

test("custom Lua and every saved setting pass through unchanged without a partial summary", async () => {
  const value = preview("new", "new", {
    master_worldgenoverride_lua: "return make_world()", master_world_size: "huge", caves_weather: "often"
  });
  assert.equal(await prepareDstWorldStart("dst", {
    flush: async () => {}, preview: async () => value
  }), value);
});
