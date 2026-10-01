const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const {
  parseInstancePortDraft,
  reconcileInstancePortDrafts
} = require("../src/views/settings/instance-port-presentation.ts");

const ports = [
  { name: "game", protocol: "udp", port: 14159 },
  { name: "query", protocol: "udp", port: 27016 }
];

test("port drafts reject partial numbers instead of truncating or reinterpreting them", () => {
  for (const value of ["", " ", "1e2", "14159.5", "123abc", "0x10", "-1", "+80", "65536"]) {
    assert.equal(parseInstancePortDraft(value, 0), null, value);
  }
  assert.equal(parseInstancePortDraft("0", 1), null);
  assert.equal(parseInstancePortDraft("0", 0), 0);
  assert.equal(parseInstancePortDraft("1", 1), 1);
  assert.equal(parseInstancePortDraft("65535", 1), 65535);
});

test("saving another port retains an invalid draft until the operator corrects it", () => {
  const draft = { "game:udp": "14159.5", "query:udp": "27017" };
  const nextPorts = [ports[0], { ...ports[1], port: 27017 }];
  const next = reconcileInstancePortDrafts(draft, ports, nextPorts);
  assert.deepEqual(next, draft);
  assert.equal(parseInstancePortDraft(next["game:udp"], 1), null);
});

test("linked port changes refresh the affected values and remove vanished bindings", () => {
  const next = reconcileInstancePortDrafts(
    { "game:udp": "", "query:udp": "27016", "removed:tcp": "bad" },
    ports,
    [{ ...ports[0], port: 14160 }, ports[1], { name: "rcon", protocol: "tcp", port: 25575 }]
  );
  assert.deepEqual(next, { "game:udp": "14160", "query:udp": "27016", "rcon:tcp": "25575" });
});
