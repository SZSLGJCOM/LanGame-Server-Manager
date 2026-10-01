const assert = require("node:assert/strict");
const test = require("node:test");
const { collectTranslationReferences, extractMessagePlaceholders } = require("../scripts/i18n-source-references.cjs");

test("translation coverage includes UiMessage calls, member t calls and literal dynamic key branches", () => {
  const references = collectTranslationReferences(`
    import { message } from "./app-ui";
    message("activity.uninstallingSteamCmd");
    props.t("common.start");
    t(failed ? "status.failed" : "status.ready");
  `, "src/example.ts");
  assert.deepEqual([...references.keys()].sort(), ["activity.uninstallingSteamCmd", "common.start", "status.failed", "status.ready"]);
  assert.deepEqual(references.get("activity.uninstallingSteamCmd"), ["src/example.ts:3"]);
});

test("translation coverage follows labeled option and plan keys consumed by the UI", () => {
  const references = collectTranslationReferences(`
    const option = { labelKey: "servers.gmTools.options.shard.master", label: "Master" };
    const plan = { summaryKey: "servers.mods.applySummary.soulmask", actionKey: "common.apply" };
    let busyLabel: Model["labelKey"];
    busyLabel = "servers.actions.repairing";
    function emptyPlan(summaryKey: string) {}
    emptyPlan("servers.mods.noSelection");
    const reasonMessageKeys = { stopped: "runtime.health.stopped" };
  `, "src/example.ts");
  assert.deepEqual([...references.keys()].sort(), [
    "common.apply", "runtime.health.stopped", "servers.actions.repairing", "servers.gmTools.options.shard.master",
    "servers.mods.applySummary.soulmask", "servers.mods.noSelection"
  ]);
});

test("translation references ignore comments, log events and configuration keys", () => {
  const references = collectTranslationReferences([
    '// t("comment.missing")',
    'const text = \'message("quoted.missing")\';',
    'logFrontendEvent("warning", "app.update.auto_check_failed");',
    'const field = { key: "scum.ServerPassword", path: "config.server.json" };',
    'function message(text: string) { return text; }',
    'message("plain.notUiMessage");',
    't(`settings.schema.${moduleId}.${fieldId}.title`);'
  ].join("\n"), "src/example.ts");
  assert.deepEqual([...references.keys()], []);
});

test("aliased UiMessage imports and Unicode line positions are recognized", () => {
  const references = collectTranslationReferences('import { message as uiMessage } from "../app-ui";\nconst title = "中文"; uiMessage("activity.failed");', "src/example.ts");
  assert.deepEqual(references.get("activity.failed"), ["src/example.ts:2"]);
});

test("placeholder comparison follows runtime interpolation including spaces and dotted parameters", () => {
  assert.deepEqual(extractMessagePlaceholders("{ count } items for {user.name}; {count}"), ["count", "count", "user.name"]);
});
