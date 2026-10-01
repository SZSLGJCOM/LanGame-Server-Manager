const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const detailPath = path.resolve(__dirname, "../src/views/servers/SteamWorkshopStoreDetail.tsx");
const windrosePath = path.resolve(__dirname, "../src/views/settings/WindroseWorldSettingsPanel.tsx");
const mediaSourcePath = path.resolve(__dirname, "../src/components/useMediaSource.ts");
const workshopStatusPath = path.resolve(__dirname, "../src/views/servers/WorkshopStatus.tsx");
let locale = "zh-CN";
let translate;
let catalogs;
const t = (key, params, fallback) => translate(locale, key, params, fallback, catalogs);

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    if ([detailPath, windrosePath, mediaSourcePath, workshopStatusPath].includes(filename)) {
      const load = module.require.bind(module);
      module.require = (request) => request === "../../i18n" || request === "../i18n"
        ? { useI18n: () => ({ locale, t }) }
        : load(request);
    }
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
({ translate } = require("../src/i18n.tsx"));
catalogs = {
  "zh-CN": require("../src/i18n-messages-zh-cn.ts").ZH_CN_MESSAGES,
  "en-US": require("../src/i18n-messages.ts").EN_US_MESSAGES
};
const { SteamWorkshopStoreDetail } = require(detailPath);
const { WindroseWorldSettingsPanel } = require(windrosePath);
const { formatWorkshopItemKind, formatWorkshopLookupStatus } = require("../src/views/servers/steam-workshop-store-model.ts");

function renderWorkshop(item = {}, overrides = {}) {
  return renderToStaticMarkup(React.createElement(SteamWorkshopStoreDetail, {
    item: { id: "1000001", title: null, item_kind: "collection", children: [{ id: "1000002", title: null }], ...item },
    lifecycleState: "installing", detailsReady: true, selected: false, batchSelectable: false,
    action: "install", actionBusy: true, actionDisabled: true, progressPercent: 42,
    onAction() {}, onOpenChild() {}, onClose() {}, onOpenExternal() {}, onToggleSelection() {}, ...overrides
  }));
}

test("Workshop unavailable titles and installation progress follow the active locale", () => {
  for (const nextLocale of ["zh-CN", "en-US"]) {
    locale = nextLocale;
    const html = renderWorkshop();
    for (const id of ["1000001", "1000002"]) {
      assert.ok(html.includes(t("servers.mods.workshopItemFallback", { id })));
    }
    assert.ok(html.includes(`aria-label="${t("servers.mods.installRunning")}"`));
    assert.match(html, /aria-valuenow="42"/);
    assert.ok(html.includes(t("servers.mods.storeDetail.collection")));
    if (locale === "zh-CN") assert.doesNotMatch(html, /Workshop item|aria-label="Installing/);
  }
});

test("Workshop localization preserves author titles and descriptions", () => {
  locale = "zh-CN";
  const html = renderWorkshop({
    title: "Author's Original Title", description: "Original description from the creator.",
    children: [{ id: "1000002", title: "Original child title" }]
  }, { lifecycleState: "downloaded", actionBusy: false });
  assert.match(html, /Author&#x27;s Original Title/);
  assert.match(html, /Original description from the creator\./);
  assert.match(html, /Original child title/);
  assert.doesNotMatch(html, /role="progressbar"/);
});

test("Workshop metrics share accessible help without duplicate native bubbles", () => {
  locale = "en-US";
  for (const actionDisabled of [false, true]) {
    const html = renderWorkshop({
      file_size: 2048, updated_at_unix: 1700000000, created_at_unix: 1600000000,
      subscriptions: 1200, favorites: 45, views: 6789, tags: ["Server", "Building"]
    }, { lifecycleState: "downloaded", actionBusy: false, actionDisabled });
    const triggers = [...html.matchAll(/<(button|span)\b[^>]*class="mw-store-detail-metric"[^>]*>/g)];
    assert.equal(triggers.length, 9, "Actions, six metrics, and tags retain their trigger surfaces");
    assert.equal(/\bdisabled=""/.test(triggers[0][0]), actionDisabled);
    for (const [trigger] of triggers) {
      assert.doesNotMatch(trigger, /\btitle=/, "A shared help trigger must not also open a native title bubble");
      const descriptionId = /aria-describedby="([^"]+)"/.exec(trigger)?.[1];
      assert.ok(descriptionId, "Every metric exposes its help to assistive technology");
      assert.ok(html.includes(`id="${descriptionId}" class="configuration-field-help-a11y" role="tooltip"`));
    }
    assert.doesNotMatch(html, /mw-store-detail-metric-tip/);
    assert.match(html, /role="tooltip">Tags Server、Building<\/span>/);
  }
});

test("Workshop details stay in a loading state until the complete response arrives", () => {
  locale = "zh-CN";
  for (const excerpt of ["Only the catalog summary is available.", undefined]) {
    const html = renderWorkshop({ description_excerpt: excerpt }, { detailsReady: false, actionBusy: false });
    assert.match(html, /class="mw-store-detail-loading[^\"]*"|class="[^\"]* mw-store-detail-loading"/);
    assert.match(html, /aria-busy="true"/);
    assert.doesNotMatch(html, /mw-store-detail-description|mw-store-detail-preview|mw-store-detail-metrics|mw-store-detail-child\b/);
    assert.ok(!html.includes(excerpt ?? t("servers.mods.storeDetail.noDescription")));
  }
});

test("Workshop application-owned kinds and lookup states have readable Chinese labels", () => {
  locale = "zh-CN";
  assert.equal(formatWorkshopItemKind("item", t), "模组");
  assert.equal(formatWorkshopItemKind("collection", t), "合集");
  for (const status of ["resolved", "not_found", "pending", "unexpected_state"]) {
    const label = formatWorkshopLookupStatus(status, t);
    assert.match(label, /\p{Script=Han}/u);
    assert.notEqual(label, status);
  }
});

function findField(element, fieldKey) {
  if (!React.isValidElement(element)) return null;
  if (element.props.fieldKey === fieldKey) return element;
  for (const child of React.Children.toArray(element.props.children)) {
    const found = findField(child, fieldKey);
    if (found) return found;
  }
  return null;
}

test("Windrose translates difficulty labels while submitting original game values", () => {
  for (const nextLocale of ["zh-CN", "en-US"]) {
    locale = nextLocale;
    const patches = [];
    const settings = { world_island_id: "world-1", world_preset_type: "Medium", combat_difficulty: "Normal" };
    const before = structuredClone(settings);
    const props = { details: { summary: { status: "Stopped" } }, settings, disabled: false, onPatch: (patch) => patches.push(patch) };
    const tree = WindroseWorldSettingsPanel(props);
    for (const [fieldKey, values] of [
      ["world_preset_type", ["Easy", "Medium", "Hard"]],
      ["combat_difficulty", ["Easy", "Normal", "Hard"]]
    ]) {
      const field = findField(tree, fieldKey);
      assert.ok(field, `${fieldKey} is present`);
      const select = field.props.children("difficulty-help");
      assert.equal(select.props.value, settings[fieldKey]);
      assert.equal(select.props.disabled, false);
      const options = React.Children.toArray(select.props.children);
      assert.deepEqual(options.map((option) => option.props.value), values);
      assert.deepEqual(options.map((option) => option.props.children), values.map((value) => t(`windrose.settings.difficulty.${value.toLowerCase()}`)));
      if (locale === "zh-CN") assert.ok(options.every((option) => /\p{Script=Han}/u.test(option.props.children)));
      select.props.onChange({ target: { value: "Hard" } });
      assert.deepEqual(patches.at(-1), { [fieldKey]: "Hard" });
    }
    assert.deepEqual(settings, before);
  }
});
