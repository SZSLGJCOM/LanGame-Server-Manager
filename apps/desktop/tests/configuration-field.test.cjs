const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const Module = require("node:module");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

const {
  ConfigurationField,
  buildConfigurationFieldIds
} = require(path.join(desktopRoot, "src", "views", "settings", "ConfigurationField.tsx"));
const {
  ConfigurationHelp,
  resolveTooltipPosition,
  summarizeConfigurationFieldHelp
} = require(path.join(desktopRoot, "src", "views", "settings", "ConfigurationFieldHelp.tsx"));
const { EN_US_MESSAGES } = require(path.join(desktopRoot, "src", "i18n-messages.ts"));
const { ZH_CN_MESSAGES } = require(path.join(desktopRoot, "src", "i18n-messages-zh-cn.ts"));

function catalogTranslator(catalog) {
  return (key, params, fallback) => String(catalog[key] ?? fallback ?? key).replace(
    /\{\s*([\w.]+)\s*\}/g,
    (match, paramKey) => params?.[paramKey] == null ? match : String(params[paramKey])
  );
}

const copy = {
  concealSecret: "Hide value",
  revealSecret: "Show value",
  restartScopes: {
    cluster: "Restart cluster",
    none: "No restart",
    server: "Restart server",
    world: "Restart world"
  }
};

function field(overrides = {}) {
  return {
    key: "server_password",
    title: "Server password",
    description: "Players must enter this password to join.",
    type: "string",
    control: "text",
    sectionId: "access",
    required: false,
    sourceId: "official-server-config",
    sourceKey: "ServerPassword",
    presentation: {
      state: "editable",
      owner: "configuration",
      sectionId: "access",
      behavior: "secret",
      restartScope: "server"
    },
    ...overrides
  };
}

function suggestionInputHarness(initialValue = "SURVIVAL_TOGETHER") {
  const filename = path.join(desktopRoot, "src", "views", "settings", "ConfigurationField.tsx");
  const hooks = [];
  let cursor = 0;
  const hookReact = {
    ...React,
    useState(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = initial;
      return [hooks[slot], (next) => {
        hooks[slot] = typeof next === "function" ? next(hooks[slot]) : next;
      }];
    },
    useRef(initial) {
      const slot = cursor++;
      if (!(slot in hooks)) hooks[slot] = { current: initial };
      return hooks[slot];
    },
    useEffect() {}
  };
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  const requireFromFile = Module.createRequire(filename);
  loaded.require = (id) => id === "react" ? hookReact : requireFromFile(id);
  const source = fs.readFileSync(filename, "utf8") + "\nexport { SuggestionInput };\n";
  loaded._compile(transpileTypeScript(source, filename), filename);
  const changes = [];
  const props = {
    accessibility: { id: "world-preset" },
    field: field({ suggestions: [
      { label: "Survival", value: "SURVIVAL_TOGETHER" },
      { label: "Endless", value: "ENDLESS" },
      { label: "Relaxed", value: "RELAXED" }
    ] }),
    toggleLabel: "Show presets",
    value: initialValue,
    onChange(value) {
      props.value = value;
      changes.push(value);
    }
  };
  function render() {
    cursor = 0;
    const root = loaded.exports.SuggestionInput(props);
    const [input, toggle] = root.props.children;
    return { root, input, toggle };
  }
  function press(key, nativeEvent = { isComposing: false }) {
    let prevented = false;
    render().input.props.onKeyDown({ key, nativeEvent, preventDefault() { prevented = true; } });
    return prevented;
  }
  return { changes, props, render, press };
}

test("typing a suggested or custom value then Enter preserves text without selecting the previous preset", () => {
  for (const text of ["ENDLESS", "MY_CUSTOM_WORLD"]) {
    const harness = suggestionInputHarness();
    harness.render().input.props.onFocus();
    harness.render().input.props.onChange({ target: { value: text } });
    assert.equal(harness.render().input.props["aria-activedescendant"], undefined);
    assert.equal(harness.press("Enter"), true);
    assert.equal(harness.props.value, text);
    assert.deepEqual(harness.changes, [text]);
    assert.equal(harness.render().input.props["aria-expanded"], false);
  }
});

test("arrow navigation explicitly selects candidates after typing in either direction", () => {
  const harness = suggestionInputHarness();
  harness.render().input.props.onChange({ target: { value: "custom" } });
  assert.equal(harness.press("ArrowDown"), true);
  assert.equal(harness.render().input.props["aria-activedescendant"], "world-preset-suggestions-option-0");
  harness.press("ArrowDown");
  assert.equal(harness.render().input.props["aria-activedescendant"], "world-preset-suggestions-option-1");
  harness.press("Enter");
  assert.equal(harness.props.value, "ENDLESS");
  assert.equal(harness.render().input.props["aria-expanded"], false);

  harness.render().input.props.onChange({ target: { value: "another" } });
  harness.press("ArrowUp");
  assert.equal(harness.render().input.props["aria-activedescendant"], "world-preset-suggestions-option-2");
  harness.press("Enter");
  assert.equal(harness.props.value, "RELAXED");
});

test("IME confirmation and navigation do not choose a preset or close suggestions", () => {
  const harness = suggestionInputHarness();
  harness.render().input.props.onChange({ target: { value: "我的世界" } });
  harness.press("ArrowDown");
  const activeBefore = harness.render().input.props["aria-activedescendant"];
  for (const key of ["Enter", "Escape", "ArrowDown", "ArrowUp", "Home", "End"]) {
    assert.equal(harness.press(key, { isComposing: true }), false);
    assert.equal(harness.render().input.props["aria-activedescendant"], activeBefore);
    assert.equal(harness.render().input.props["aria-expanded"], true);
  }
  assert.deepEqual(harness.changes, ["我的世界"]);
});

test("Tab keeps internal focus usable and leaving the combobox closes and clears navigation", () => {
  const harness = suggestionInputHarness();
  const toggle = {};
  const currentTarget = { contains(target) { return target === toggle; } };
  harness.render().input.props.onFocus();
  harness.press("ArrowDown");
  assert.equal(harness.press("Tab"), false);
  harness.render().root.props.onBlur({ currentTarget, relatedTarget: toggle });
  assert.equal(harness.render().input.props["aria-expanded"], true);
  harness.render().root.props.onBlur({ currentTarget, relatedTarget: {} });
  assert.equal(harness.render().input.props["aria-expanded"], false);
  harness.render().input.props.onFocus();
  assert.equal(harness.render().input.props["aria-activedescendant"], undefined);
  harness.render().root.props.onBlur({ currentTarget, relatedTarget: null });
  assert.equal(harness.render().input.props["aria-expanded"], false);
  assert.deepEqual(harness.changes, []);
});

test("field help omits missing text, same-label echoes and provenance-only sentences", () => {
  const en = catalogTranslator(EN_US_MESSAGES);
  const zh = catalogTranslator(ZH_CN_MESSAGES);
  for (const description of [
    "控制 PvE 模式。此项会写入 ARK 的实例配置文件。",
    "Written to ServerSettings.ini.",
    "此项会写入服务器原生配置文件。",
    "This value is stored in the native configuration file.",
    "从等级倍率派生该值，供 Dedicated Server 读取。",
    "Native ARK server setting RCONServerGameLogBuffer.",
    "Native balatro option from Klei DST build 740477.",
    "Conan Exiles Enhanced build 23249060 native server setting.",
    "Passed as -PSW.",
    "作为 -PSW 参数传入。",
    undefined, "   "
  ]) {
    for (const t of [en, zh]) assert.equal(summarizeConfigurationFieldHelp(description, "PvE 模式", t), null, description);
  }
  assert.equal(summarizeConfigurationFieldHelp("Controls friendly fire. This value is written to Server.ini.", "Friendly fire"),
    "Controls friendly fire.");
});

test("field help preserves units, conditions and identifiers that explain an actual choice", () => {
  for (const description of [
    "Multiplier for damage to player structures. Values below 1 reduce damage; 0 disables damage.",
    "Selects the existing IslandId. Leave empty to create a new world.",
    "ServerFPS limits frames per second. Lowering the limit can reduce CPU usage.",
    "Native interval setting for Rift Incursions; this value is not a duration in seconds.",
    "Written as a multiplier; 0 disables damage.",
    "设为 0.5 时获得的经验减半。此设置在下次启动时生效。",
    "原生参数 UpgradeOptionalDLC.Array 用于升级旧世界。升级后，没有对应 DLC 的玩家无法加入。"
  ]) assert.equal(summarizeConfigurationFieldHelp(description, "Setting"), description);
  assert.equal(summarizeConfigurationFieldHelp("Written to Server.ini. Set to 0 to disable the timeout.", "Timeout"),
    "Set to 0 to disable the timeout.");
});

test("instruction mode preserves complete rule syntax and adds no placeholder", () => {
  for (const description of [
    "One native removal tuple per line. NPCSpawnEntries uses NPCsToSpawnStrings with full blueprint paths ending in _C. NPCSpawnLimits uses NPCClassString.",
    "每行填写一个索引倍率，如 [0]=1.0、_Add[0]=1.0 或 _Affinity[0]=1.0。可填写完整 PerLevelStatsMultiplier_DinoTamed 键；留空时沿用游戏默认值。"
  ]) {
    assert.equal(summarizeConfigurationFieldHelp(description, "Rules", undefined, "instructions"), description);
    assert.equal(summarizeConfigurationFieldHelp(description, "Rules"), description);
  }
  assert.equal(summarizeConfigurationFieldHelp(" ", "Rules", undefined, "instructions"), null);
});

test("multiline and raw controls retain all instructions in their accessible tooltip", () => {
  const description = "One ConfigOverrideItemCraftingCosts rule per line. The key prefix is optional; leave blank to use game defaults.";
  for (const behavior of ["multiline", "raw"]) {
    const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
      copy,
      field: field({
        key: "crafting_rules",
        title: "Crafting rules",
        control: "textarea",
        description,
        presentation: { state: "editable", owner: "configuration", sectionId: "crafting", behavior }
      }),
      onPatch: () => undefined,
      settings: {},
      value: ""
    }));
    assert.match(html, /<textarea[^>]*aria-describedby="configuration-crafting-rules-description"/);
    assert.match(html, /id="configuration-crafting-rules-description"[^>]*role="tooltip"/);
    assert.ok(html.includes(description));
    assert.doesNotMatch(html, /Controls Crafting rules for this server/);
  }

  const summaryHtml = renderToStaticMarkup(React.createElement(ConfigurationField, {
    copy,
    field: field({ description }),
    onPatch: () => undefined,
    settings: {},
    value: ""
  }));
  assert.match(summaryHtml, /role="tooltip"/);
  assert.ok(summaryHtml.includes(description));
  assert.match(summaryHtml, /ConfigOverrideItemCraftingCosts/);
});

test("field help uses a clipped accessible node and a portal tooltip instead of browser title", () => {
  const fieldSource = fs.readFileSync(
    path.join(desktopRoot, "src", "views", "settings", "ConfigurationField.tsx"),
    "utf8"
  );
  const helpSource = fs.readFileSync(
    path.join(desktopRoot, "src", "views", "settings", "ConfigurationFieldHelp.tsx"),
    "utf8"
  );
  const styles = fs.readFileSync(
    path.join(desktopRoot, "src", "styles", "configuration-field-help.css"),
    "utf8"
  );

  assert.doesNotMatch(fieldSource, /\btitle\s*=\s*[{\"]/);
  assert.doesNotMatch(helpSource, /\btitle\s*=\s*[{\"]/);
  assert.match(helpSource, /createPortal\s*\(/);
  assert.match(helpSource, /role="tooltip"/);
  assert.match(styles, /\.configuration-field-help-a11y\s*{/);
  assert.match(styles, /\.configuration-field-help-tooltip\s*{[^}]*position:\s*fixed;/s);
  assert.match(styles, /\.configuration-field-help-tooltip\s*{[^}]*pointer-events:\s*auto;/s);
  assert.match(styles, /\.configuration-field-help-tooltip\s*{[^}]*overflow-wrap:\s*anywhere;/s);
  assert.match(styles, /\.configuration-field-help-tooltip\s*{[^}]*white-space:\s*pre-wrap;/s);
  assert.match(styles, /\.configuration-field-help-tooltip\s*{[^}]*max-height:\s*calc\(100vh - 16px\);[^}]*overflow-y:\s*auto;/s);
  assert.match(styles, /\.configuration-field-help-tooltip\s*{[^}]*opacity:\s*0;[^}]*translateY\(-4px\)/s);
  assert.match(styles, /\.configuration-field-help-tooltip\.is-visible\s*{[^}]*opacity:\s*1;[^}]*translateY\(0\)/s);
  assert.match(styles, /\.configuration-field-help-tooltip\.is-leaving\s*{[^}]*opacity:\s*0;[^}]*translateY\(4px\)/s);
  assert.match(styles, /@media\s*\(prefers-reduced-motion:\s*reduce\)/);
  assert.doesNotMatch(styles, /translateY\(-100%\)/);
});

test("field help stays below its control unless the viewport requires a nearby flip", () => {
  const regular = resolveTooltipPosition(
    { left: 400, right: 700, top: 300, bottom: 360 },
    { width: 240, height: 48 },
    { width: 1200, height: 800 }
  );
  assert.deepEqual(regular, {
    left: 400,
    maxWidth: 320,
    placement: "below",
    top: 368
  });

  const nearBottom = resolveTooltipPosition(
    { left: 400, right: 700, top: 650, bottom: 710 },
    { width: 240, height: 80 },
    { width: 1200, height: 760 }
  );
  assert.equal(nearBottom.placement, "above");
  assert.equal(nearBottom.top, 562);
  assert.equal(650 - (nearBottom.top + 80), 8);

  const rightEdge = resolveTooltipPosition(
    { left: 1100, right: 1180, top: 300, bottom: 360 },
    { width: 260, height: 48 },
    { width: 1200, height: 800 }
  );
  assert.equal(rightEdge.left, 932);

  const narrow = resolveTooltipPosition(
    { left: 100, right: 135, top: 20, bottom: 40 },
    { width: 320, height: 48 },
    { width: 140, height: 120 }
  );
  assert.equal(narrow.maxWidth, 124);
  assert.equal(narrow.left, 8);
});

test("action and path help preserves complete instructions and the existing control element", () => {
  const description = "D:/Server data/world-with-a-long-unbroken-name/save\nStop the server before replacing <world>.";
  const html = renderToStaticMarkup(React.createElement(ConfigurationHelp, { description }, (help) =>
    React.createElement("button", {
      className: "existing-action", ref: help.anchorRef, ...help.interactionProps,
      "aria-describedby": help.descriptionId
    }, "Open folder")
  ));
  assert.match(html, /^<button class="existing-action" aria-describedby="([^"]+)">Open folder<\/button><span id="\1"/);
  assert.match(html, /role="tooltip">D:\/Server data\/world-with-a-long-unbroken-name\/save\nStop the server before replacing &lt;world&gt;\.<\/span>$/);
  assert.doesNotMatch(html, /title=|<world>/);

  const withoutHelp = renderToStaticMarkup(React.createElement(ConfigurationHelp, {}, (help) =>
    React.createElement("button", { "aria-describedby": help.descriptionId }, "Refresh")
  ));
  assert.equal(withoutHelp, "<button>Refresh</button>");
});

test("field help is tooltip-only while validation, secrets, and restart impact stay visible", () => {
  const ids = buildConfigurationFieldIds("Server Password", "configuration-main");
  assert.deepEqual(ids, {
    descriptionId: "configuration-main-server-password-description",
    errorId: "configuration-main-server-password-error",
    inputId: "configuration-main-server-password-input"
  });

  const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
    copy,
    field: field({
      description: "Used by players to join the server. This value is written to the native authentication file."
    }),
    idPrefix: "configuration-main",
    onPatch: () => undefined,
    settings: {},
    validationMessage: "Password is required.",
    value: "secret"
  }));

  assert.match(html, /for="configuration-main-server-password-input"/);
  assert.match(html, /id="configuration-main-server-password-input"/);
  assert.match(html, /aria-describedby="configuration-main-server-password-description"/);
  assert.match(html, /aria-invalid="true"/);
  assert.match(html, /aria-errormessage="configuration-main-server-password-error"/);
  assert.match(html, /id="configuration-main-server-password-description"[^>]*role="tooltip"[^>]*>Used by players to join the server\./);
  assert.match(html, /id="configuration-main-server-password-error"[^>]*>Password is required\./);
  assert.match(html, /type="password"/);
  assert.match(html, /aria-pressed="false"/);
  assert.match(html, />Show value</);
  assert.match(html, /class="configuration-field-restart-scope"[^>]*>Restart server</);
  assert.doesNotMatch(html, /This value is written to the native authentication file/);
  assert.doesNotMatch(html, /configuration-field-evidence/);
  assert.doesNotMatch(html, /ServerPassword/);
  assert.doesNotMatch(html, /official-server-config/);
  assert.doesNotMatch(html, />Native key</);
  assert.doesNotMatch(html, />Source</);
  assert.doesNotMatch(html, /class="form-note"[^>]*>Used by players/);
  assert.doesNotMatch(html, /\stitle=/);
});

test("fields without authored help do not repeat their label in a tooltip", () => {
  const en = catalogTranslator(EN_US_MESSAGES);
  const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
    copy,
    field: field({ description: undefined }),
    onPatch: () => undefined,
    settings: {},
    t: en,
    value: "secret"
  }));

  assert.doesNotMatch(html, /role="tooltip"/);
  assert.doesNotMatch(html, /aria-describedby=/);
  assert.doesNotMatch(html, /configuration-field-evidence/);
  assert.doesNotMatch(html, /ServerPassword/);
  assert.doesNotMatch(html, /official-server-config/);
  assert.doesNotMatch(html, /\stitle=/);
});

test("field help removes only the active locale's exact same-label template", () => {
  for (const [catalog, title, template, useful] of [
    [EN_US_MESSAGES, "Experience rate", "Controls “Experience rate” for this server.", "Use 0.5 for half the normal experience gain."],
    [ZH_CN_MESSAGES, "经验倍率", "控制此服务器的「经验倍率」。", "设为 0.5 时获得的经验减半。"]
  ]) {
    const t = catalogTranslator(catalog);
    const render = (description, label = title) => renderToStaticMarkup(React.createElement(ConfigurationField, {
      copy, field: field({ title: label, description }), onPatch() {}, settings: {}, t, value: ""
    }));
    assert.doesNotMatch(render(template), /role="tooltip"/);
    assert.match(render(template, `${title} (override)`), /role="tooltip"/);
    assert.ok(render(useful).includes(useful));
    const withConstraint = render(`${template} ${useful}`);
    assert.match(withConstraint, /role="tooltip"/);
    assert.ok(withConstraint.includes(useful));
    assert.ok(!withConstraint.includes(template));
  }
  const technical = renderToStaticMarkup(React.createElement(ConfigurationField, {
    copy, field: field({ description: "Written to ServerSettings.ini." }), onPatch() {}, settings: {}, value: ""
  }));
  assert.doesNotMatch(technical, /role="tooltip"/);
});

test("ARK authored descriptions remain accessible without expanding checkbox labels", () => {
  for (const catalog of [ZH_CN_MESSAGES, EN_US_MESSAGES]) {
    const t = catalogTranslator(catalog);
    for (const key of ["allow_cave_building_pve", "override_structure_platform_prevention"]) {
      const title = t(`settings.schema.arksurvivalascended.${key}.title`);
      const description = t(`settings.schema.arksurvivalascended.${key}.description`);
      const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
        copy, field: field({ key, title, description, control: "checkbox", type: "boolean", defaultValue: false,
          presentation: { state: "editable", owner: "configuration", behavior: "plain" } }),
        onPatch() {}, settings: {}, t, value: false
      }));
      assert.match(html, /<label[^>]*settings-toggle-card[^>]*>[\s\S]*type="checkbox"[\s\S]*<\/label>/);
      assert.match(html, /type="checkbox"/);
      assert.match(html, /role="tooltip"/);
      assert.ok(html.includes(description));
      assert.ok(!html.match(/<label[\s\S]*?<\/label>/)?.[0].includes(description));
    }
    const key = "settings.schema.arksurvivalascended.prevent_template_on_saddle";
    const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
      copy, field: field({ title: t(`${key}.title`), description: t(`${key}.description`) }),
      onPatch() {}, settings: {}, t, value: ""
    }));
    assert.match(html, /role="tooltip"/);
    assert.ok(html.includes(catalog === ZH_CN_MESSAGES
      ? "禁止在平台鞍上放置建筑模板。" : "Prevents placing building templates on platform saddles."));
  }
});

test("same-label template matching folds only ASCII case and retains distinct labels and constraints", () => {
  const t = catalogTranslator(ZH_CN_MESSAGES);
  const render = (title, description) => renderToStaticMarkup(React.createElement(ConfigurationField, {
    copy, field: field({ title, description }), onPatch() {}, settings: {}, t, value: ""
  }));
  assert.doesNotMatch(render("PVE 允许洞穴建造", "控制 PvE 允许洞穴建造。"), /role="tooltip"/);
  const mapTitle = t("settings.schema.arksurvivalascended.map_name.title");
  const mapDescription = t("settings.schema.arksurvivalascended.map_name.description");
  assert.match(render(mapTitle, mapDescription), /role="tooltip"/);
  assert.ok(render(mapTitle, mapDescription).includes(mapDescription));
  const constraint = "每个氏族最多可放置 12 个建筑。";
  const combined = render("PVE 允许洞穴建造", `控制 PvE 允许洞穴建造。${constraint}`);
  assert.ok(combined.includes(constraint));
  assert.ok(!combined.includes("控制 PvE 允许洞穴建造。"));
  for (const [title, description] of [
    ["地图名称", "控制 地图大小。"],
    ["更新频率", "控制 更新频率，范围为 10–60 Hz。"],
    ["经验倍率", "控制 经验倍率（单位：倍）。"],
    ["PVE", "控制 PVÉ。"]
  ]) {
    const html = render(title, description);
    assert.match(html, /role="tooltip"/);
    assert.ok(html.includes(description));
  }
});

test("secret rendering is explicit and path names are never inferred", () => {
  const pathField = field({
    key: "password_store_path",
    title: "Password store path",
    presentation: {
      state: "editable",
      owner: "configuration",
      sectionId: "runtime",
      behavior: "path"
    }
  });
  const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
    copy,
    field: pathField,
    onPatch: () => undefined,
    settings: {},
    value: "D:/server/passwords"
  }));

  assert.match(html, /type="text"/);
  assert.doesNotMatch(html, /type="password"/);
  assert.doesNotMatch(html, /aria-pressed=/);

  const secretWithSuggestions = renderToStaticMarkup(React.createElement(ConfigurationField, {
    copy,
    field: field({ suggestions: [{ label: "Unsafe suggestion", value: "plain-secret" }] }),
    onPatch: () => undefined,
    settings: {},
    value: "still-secret"
  }));
  assert.match(secretWithSuggestions, /type="password"/);
  assert.doesNotMatch(secretWithSuggestions, /role="combobox"/);
});

test("suggestion inputs expose a connected keyboard-driven combobox", () => {
  const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
    copy,
    field: field({
      key: "map_name",
      presentation: { state: "editable", owner: "configuration", sectionId: "world" },
      suggestions: [{ label: "The Island", value: "TheIsland" }]
    }),
    onPatch: () => undefined,
    settings: {},
    value: ""
  }));
  assert.match(html, /role="combobox"/);
  assert.match(html, /aria-autocomplete="list"/);
  assert.match(html, /aria-controls="configuration-map-name-input-suggestions"/);
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "settings", "ConfigurationField.tsx"), "utf8");
  assert.match(source, /aria-activedescendant/);
  assert.match(source, /event\.key === "ArrowDown" \|\| event\.key === "ArrowUp"/);
  assert.match(source, /event\.key === "Enter"/);
});

test("scalar, enum, multiline, raw, and specialized presentations have deliberate controls", () => {
  const cases = [
    [field({ key: "enabled", control: "checkbox", type: "boolean", defaultValue: false, presentation: { state: "editable", owner: "configuration", sectionId: "room", behavior: "plain" } }), /type="checkbox"/],
    [field({ key: "mode", control: "select", enumOptions: [{ label: "Public", value: "public" }], presentation: { state: "editable", owner: "configuration", sectionId: "room", behavior: "plain" } }), /<select/],
    [field({ key: "motd", control: "textarea", presentation: { state: "editable", owner: "configuration", sectionId: "room", behavior: "multiline" } }), /data-configuration-behavior="multiline"/],
    [field({ key: "server_cfg", control: "textarea", presentation: { state: "editable", owner: "configuration", sectionId: "advanced", behavior: "raw" } }), /data-configuration-behavior="raw"/]
  ];
  for (const [candidate, expected] of cases) {
    const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
      copy,
      field: candidate,
      onPatch: () => undefined,
      settings: {},
      value: candidate.type === "boolean" ? true : "public"
    }));
    assert.match(html, expected, candidate.key);
  }

  let receivedProps;
  let receivedPatch;
  function SpecializedRenderer(props) {
    receivedProps = props;
    props.onPatch({ structured_value: "next" });
    return React.createElement("div", { id: props.inputId }, "Specialized");
  }
  const specialized = field({
    key: "structured_value",
    presentation: {
      state: "specialized",
      owner: "configuration",
      sectionId: "advanced",
      rendererId: "structured-editor"
    }
  });
  renderToStaticMarkup(React.createElement(ConfigurationField, {
    copy,
    field: specialized,
    onPatch: (patch) => { receivedPatch = patch; },
    renderSpecialized: SpecializedRenderer,
    settings: { unrelated: true },
    value: "before"
  }));
  assert.deepEqual(receivedPatch, { structured_value: "next" });
  assert.equal(typeof receivedProps.onPatch, "function");
  assert.equal(receivedProps.onChange, undefined);
  assert.equal(receivedProps.onSettingsChange, undefined);
});

test("boolean cards remain wholly label-clickable, use tooltip help, and keep checklist groups named", () => {
  const booleanHtml = renderToStaticMarkup(React.createElement(ConfigurationField, {
    copy,
    field: field({ key: "enabled", control: "checkbox", type: "boolean", defaultValue: false, presentation: { state: "editable", owner: "configuration", sectionId: "room", behavior: "plain" } }),
    onPatch: () => undefined,
    settings: {},
    value: true
  }));
  assert.match(booleanHtml, /<label[^>]*settings-toggle-card[^>]*for="configuration-enabled-input"[^>]*>[\s\S]*id="configuration-enabled-input"[^>]*type="checkbox"[\s\S]*<\/label>/);
  assert.match(booleanHtml, /for="configuration-enabled-input"/);
  assert.match(booleanHtml, /class="settings-toggle-copy"/);
  assert.match(booleanHtml, /id="configuration-enabled-input"[^>]*type="checkbox"/);
  assert.match(booleanHtml, /aria-describedby="configuration-enabled-description"/);
  assert.match(booleanHtml, /id="configuration-enabled-description"[^>]*role="tooltip"/);
  assert.doesNotMatch(booleanHtml, /configuration-field-evidence/);
  assert.doesNotMatch(booleanHtml, /ServerPassword/);
  assert.doesNotMatch(booleanHtml, /official-server-config/);
  assert.doesNotMatch(booleanHtml, /class="form-note"/);
  assert.doesNotMatch(booleanHtml, /\stitle=/);

  const checklistHtml = renderToStaticMarkup(React.createElement(ConfigurationField, {
    copy,
    field: field({
      key: "permissions",
      editorVariant: "enum-check-list",
      presentation: { state: "editable", owner: "configuration", sectionId: "access", behavior: "plain" },
      suggestions: [{ label: "Kick", value: "kick" }]
    }),
    onPatch: () => undefined,
    settings: {},
    value: "kick"
  }));
  assert.match(checklistHtml, /role="group"/);
  assert.match(checklistHtml, /aria-labelledby="configuration-permissions-input-label"/);
  assert.match(checklistHtml, /id="configuration-permissions-input-label"/);
});

test("non-editable fields and missing specialized renderers fail closed", () => {
  for (const candidate of [
    field({
      key: "generated_id",
      presentation: {
        state: "generated",
        owner: "configuration",
        sectionId: "advanced",
        reason: "Generated from instance identity."
      }
    }),
    field({
      key: "structured_missing",
      presentation: {
        state: "specialized",
        owner: "configuration",
        sectionId: "advanced",
        rendererId: "missing-renderer"
      }
    })
  ]) {
    const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
      copy: { ...copy, specializedUnavailable: "Specialized editor unavailable." },
      field: candidate,
      onPatch: () => undefined,
      settings: {},
      value: "must-not-render-as-input"
    }));
    assert.match(html, /configuration-field-unavailable/);
    assert.match(html, /role="status"/);
    assert.doesNotMatch(html, /<input/);
    assert.doesNotMatch(html, /<textarea/);
  }
});

test("GuidedSettingsForm delegates field markup without recreating public evidence", () => {
  const source = fs.readFileSync(
    path.join(desktopRoot, "src", "views", "settings", "GuidedSettingsForm.tsx"),
    "utf8"
  );
  assert.match(source, /<ConfigurationField/);
  assert.doesNotMatch(source, /password\|token\|secret\|key/);
  assert.doesNotMatch(source, /settings\.configuration\.evidence\.(?:nativeKey|source)/);
});
