const test = require("node:test");
const assert = require("node:assert/strict");
const { recordTree } = require("../scripts/verify_desktop_reliability.cjs");

const processRow = (ProcessId, ParentProcessId, Created) => ({ ProcessId, ParentProcessId, Created });

test("native cleanup still observes children first sampled after their owner exits", () => {
  const observed = new Map();
  recordTree([processRow(42, 1, "100"), processRow(43, 42, "110")], 42, observed);
  recordTree([processRow(44, 43, "120"), processRow(45, 44, "130")], 42, observed);
  assert.deepEqual([...observed.keys()], [42, 43, 44, 45]);
});

test("reused host and browser PIDs cannot adopt an unrelated process tree", () => {
  const observed = new Map([[42, "100"], [43, "110"]]);
  recordTree([processRow(42, 1, "200"), processRow(43, 42, "210"), processRow(44, 43, "220")], 42, observed);
  assert.deepEqual([...observed], [[42, "100"], [43, "110"]]);
});

test("an observed parent cannot claim a process born before it", () => {
  const observed = new Map([[42, "100"]]);
  recordTree([processRow(50, 42, "90")], 42, observed);
  assert.equal(observed.has(50), false);
});
