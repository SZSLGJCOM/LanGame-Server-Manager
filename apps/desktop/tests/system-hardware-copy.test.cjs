const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const volumePath = path.join(__dirname, "../src/domain/system-volume-label.ts");
const volumeScope = { exports: {} };
vm.runInNewContext(transpileTypeScript(fs.readFileSync(volumePath, "utf8"), volumePath), {
  module: volumeScope, exports: volumeScope.exports, require
}, { filename: volumePath });
const sourcePath = path.join(__dirname, "../src/views/system/system-hardware-copy.ts");
const scope = { exports: {} };
vm.runInNewContext(transpileTypeScript(fs.readFileSync(sourcePath, "utf8"), sourcePath), {
  module: scope, exports: scope.exports, require: (name) => name === "../../domain/system-volume-label" ? volumeScope.exports : require(name)
}, { filename: sourcePath });
const { memoryHardwareCopy, diskHardwareCopy } = scope.exports;
const { displayVolumePath } = volumeScope.exports;
const moduleSample = (overrides = {}) => ({ manufacturer: "Kingston", part_number: "KF560C36-16",
  configured_clock_mts: 4800, speed_mts: 6000, memory_type: "DDR5", ...overrides });

test("memory reports its configured data rate and deduplicates identical modules", () => {
  const actual = memoryHardwareCopy([moduleSample(), moduleSample()]);
  assert.equal(actual.model, "Kingston KF560C36-16");
  assert.equal(actual.speed, "4800 MT/s");
});

test("memory uses rated speed only when configured speed is absent and never infers manufacturer", () => {
  const actual = memoryHardwareCopy([moduleSample({ manufacturer: "Unknown", configured_clock_mts: 0 })]);
  assert.equal(actual.model, "KF560C36-16");
  assert.equal(actual.speed, "6000 MT/s");
  assert.equal(memoryHardwareCopy(undefined).model, null);
  assert.equal(memoryHardwareCopy(null).speed, null);
  assert.equal(memoryHardwareCopy([moduleSample({ manufacturer: "To Be Filled By O.E.M.", part_number: "N/A",
    configured_clock_mts: 0, speed_mts: 0 })]).model, "DDR5");
});

test("mixed memory modules retain their distinct models and actual data rates", () => {
  const actual = memoryHardwareCopy([moduleSample(), moduleSample({ manufacturer: "Other", part_number: "Part-2",
    configured_clock_mts: 5600 })]);
  assert.equal(actual.model, "Kingston KF560C36-16 + Other Part-2");
  assert.equal(actual.speed, "4800 / 5600 MT/s");
});

test("disk copy prefers a supplied physical model and removes extended path syntax from fallback", () => {
  assert.equal(diskHardwareCopy("Samsung SSD 990 PRO 1TB", "\\\\?\\C:\\"), "Samsung SSD 990 PRO 1TB");
  assert.equal(diskHardwareCopy(null, "\\\\?\\D:\\"), "D:");
  assert.equal(displayVolumePath("C:/Mount/Games/"), "C:\\Mount\\Games");
});

test("disk copy never exposes volume GUIDs as device names", () => {
  const guid = "\\\\?\\Volume{aabbccdd-1234-5678-abcd-123456789abc}\\";
  assert.equal(diskHardwareCopy(null, guid), null);
  assert.equal(diskHardwareCopy(guid, "D:"), "D:");
  assert.equal(diskHardwareCopy(null, guid, ["D:\\"]), "D:");
});
