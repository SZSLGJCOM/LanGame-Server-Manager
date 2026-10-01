const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  const outputText = transpileTypeScript(source, filename);
  module._compile(outputText, filename);
};

const {
  filterLibraryCatalogModules,
  resolveLibraryCatalogAlignmentScrollLeft,
  resolveLibraryCatalogFocusId,
  resolveLibraryCatalogPageFocusId
} = require(path.join(desktopRoot, "src", "views", "library", "library-catalog-model.ts"));
const {
  canConfirmLibraryCatalogPointerSelection,
  createLibraryGamepadState,
  libraryCatalogOptionId,
  resolveLibraryGamepadAxisDirection,
  resolveLibraryHorizontalFocusId,
  resolveLibraryKeyboardAction,
  stepLibraryGamepad
} = require(path.join(desktopRoot, "src", "views", "library", "library-navigation-model.ts"));

function moduleSummary(id, overrides = {}) {
  return {
    id,
    name: id,
    version: "1.0.0",
    description: "",
    install_state: "notinstalled",
    supported_platforms: ["windows"],
    steam_app_id: null,
    ...overrides
  };
}

const modules = [
  moduleSummary("minecraft", {
    name: "Minecraft",
    steam_app_id: null,
    install_state: "installed"
  }),
  moduleSummary("palworld", {
    name: "Palworld",
    description: "Host a creature survival world.",
    steam_app_id: 2394010
  }),
  moduleSummary("terraria", {
    name: "Terraria",
    steam_app_id: 105600,
    install_state: "installed"
  })
];

const storeEntries = {
  minecraft: {
    storeName: "Minecraft Java",
    shortDescription: "Blocks and survival.",
    genres: ["Sandbox"]
  },
  palworld: {
    storeName: "幻兽帕鲁 Palworld",
    shortDescription: "Open world survival crafting.",
    genres: ["Survival", "Creature Collector"]
  },
  terraria: {
    storeName: "Terraria",
    shortDescription: "2D sandbox adventure.",
    genres: ["Adventure"]
  }
};

function resolveStoreEntry(moduleId) {
  return storeEntries[moduleId] ?? null;
}

function gamepadSnapshot({ axes = [0, 0], pressed = [], connected = true } = {}) {
  const buttons = Array.from({ length: 16 }, () => false);
  for (const index of pressed) {
    buttons[index] = true;
  }
  return { axes, buttons, connected };
}

test("library catalog search matches game metadata without changing catalog order", () => {
  assert.deepEqual(
    filterLibraryCatalogModules(modules, "帕鲁", resolveStoreEntry).map((module) => module.id),
    ["palworld"]
  );
  assert.deepEqual(
    filterLibraryCatalogModules(modules, "105600", resolveStoreEntry).map((module) => module.id),
    ["terraria"]
  );
  assert.deepEqual(
    filterLibraryCatalogModules(modules, "survival", resolveStoreEntry).map((module) => module.id),
    ["minecraft", "palworld"]
  );
  assert.strictEqual(filterLibraryCatalogModules(modules, "", resolveStoreEntry), modules);
});

test("library catalog keeps an ordinary empty result after storage is ready", () => {
  assert.deepEqual(filterLibraryCatalogModules([], "", resolveStoreEntry), []);
  assert.deepEqual(filterLibraryCatalogModules(modules, "missing-game", resolveStoreEntry), []);
});

test("library catalog focus restores a visible remembered game and falls back deterministically", () => {
  assert.equal(resolveLibraryCatalogFocusId(modules, "terraria", "minecraft"), "terraria");
  assert.equal(resolveLibraryCatalogFocusId(modules, "missing", "palworld"), "palworld");
  assert.equal(resolveLibraryCatalogFocusId(modules, null, null), "minecraft");
  assert.equal(resolveLibraryCatalogFocusId([], "terraria", "minecraft"), null);
});

test("horizontal rail page navigation retains one overlap card and clamps at its edges", () => {
  const manyModules = Array.from({ length: 8 }, (_, index) => moduleSummary(`game-${index + 1}`));

  assert.equal(resolveLibraryCatalogPageFocusId(manyModules, "game-1", "right", 4), "game-4");
  assert.equal(resolveLibraryCatalogPageFocusId(manyModules, "game-4", "right", 4), "game-7");
  assert.equal(resolveLibraryCatalogPageFocusId(manyModules, "game-7", "right", 4), "game-8");
  assert.equal(resolveLibraryCatalogPageFocusId(manyModules, "game-7", "left", 4), "game-4");
  assert.equal(resolveLibraryCatalogPageFocusId(manyModules, "game-4", "left", 4), "game-1");
  assert.equal(resolveLibraryCatalogPageFocusId([], "game-2", "right", 4), null);
});

test("horizontal rail alignment leaves safe cards still and moves only enough to reveal clipped cards", () => {
  assert.equal(
    resolveLibraryCatalogAlignmentScrollLeft({
      railLeft: 100,
      railRight: 900,
      railScrollLeft: 240,
      tileLeft: 360,
      tileRight: 560,
      maxScrollLeft: 1200,
      safeInset: 72
    }),
    null
  );
  assert.equal(
    resolveLibraryCatalogAlignmentScrollLeft({
      railLeft: 100,
      railRight: 900,
      railScrollLeft: 240,
      tileLeft: 780,
      tileRight: 980,
      maxScrollLeft: 1200,
      safeInset: 72
    }),
    392
  );
  assert.equal(
    resolveLibraryCatalogAlignmentScrollLeft({
      railLeft: 100,
      railRight: 900,
      railScrollLeft: 240,
      tileLeft: -80,
      tileRight: 120,
      maxScrollLeft: 1200,
      safeInset: 72
    }),
    0
  );
});

test("catalog option ids escape module ids", () => {
  assert.equal(
    libraryCatalogOptionId({ moduleId: "space game/1" }),
    "library-catalog-space%20game%2F1"
  );
});

test("pointer confirmation waits until an already selected card has settled", () => {
  assert.equal(canConfirmLibraryCatalogPointerSelection(false, null, 1000), false);
  assert.equal(canConfirmLibraryCatalogPointerSelection(true, 800, 1000), false);
  assert.equal(canConfirmLibraryCatalogPointerSelection(true, 740, 1000), true);
  assert.equal(canConfirmLibraryCatalogPointerSelection(true, null, 1000), false);
});

test("keyboard inputs map to one selection, paging, confirmation, back or context action", () => {
  assert.deepEqual(resolveLibraryKeyboardAction({ key: "ArrowLeft" }), {
    type: "move",
    direction: "left",
    source: "keyboard"
  });
  assert.deepEqual(resolveLibraryKeyboardAction({ key: "PageDown" }), {
    type: "page",
    direction: "right",
    source: "keyboard"
  });
  assert.deepEqual(resolveLibraryKeyboardAction({ key: "Enter" }), {
    type: "confirm",
    source: "keyboard"
  });
  assert.deepEqual(resolveLibraryKeyboardAction({ key: "Escape" }), {
    type: "back",
    source: "keyboard"
  });
  assert.deepEqual(resolveLibraryKeyboardAction({ key: "ContextMenu" }), {
    type: "context",
    source: "keyboard"
  });
  assert.deepEqual(resolveLibraryKeyboardAction({ key: "F10", shiftKey: true }), {
    type: "context",
    source: "keyboard"
  });
  assert.deepEqual(resolveLibraryKeyboardAction({ key: "ArrowLeft", altKey: true }), {
    type: "back",
    source: "keyboard"
  });
  assert.equal(resolveLibraryKeyboardAction({ key: "Enter", repeat: true }), null);
  assert.equal(resolveLibraryKeyboardAction({ key: "ArrowRight", ctrlKey: true }), null);
});

test("horizontal navigation clamps at both rail boundaries", () => {
  const ids = ["a", "b", "c", "d", "e", "f", "g"];

  assert.equal(resolveLibraryHorizontalFocusId(ids, "a", "left"), "a");
  assert.equal(resolveLibraryHorizontalFocusId(ids, "g", "right"), "g");
  assert.equal(resolveLibraryHorizontalFocusId(ids, "c", "right"), "d");
  assert.equal(resolveLibraryHorizontalFocusId(ids, "c", "left"), "b");
  assert.equal(resolveLibraryHorizontalFocusId([], "a", "right"), null);
});

test("gamepad axes use press and release thresholds to prevent focus chatter", () => {
  assert.equal(resolveLibraryGamepadAxisDirection([0.61, 0]), null);
  assert.equal(resolveLibraryGamepadAxisDirection([0.63, 0]), "right");
  assert.equal(resolveLibraryGamepadAxisDirection([0.5, 0], "right"), "right");
  assert.equal(resolveLibraryGamepadAxisDirection([0.41, 0], "right"), null);
  assert.equal(resolveLibraryGamepadAxisDirection([0.7, -0.8], "right"), "up");
});

test("gamepad movement repeats after 320 ms and then at 100 ms intervals", () => {
  let result = stepLibraryGamepad(createLibraryGamepadState(), gamepadSnapshot({ axes: [0.8, 0] }), 1000);
  assert.deepEqual(result.actions, [{ type: "move", direction: "right", source: "gamepad" }]);

  result = stepLibraryGamepad(result.state, gamepadSnapshot({ axes: [0.8, 0] }), 1319);
  assert.deepEqual(result.actions, []);
  result = stepLibraryGamepad(result.state, gamepadSnapshot({ axes: [0.8, 0] }), 1320);
  assert.deepEqual(result.actions, [{ type: "move", direction: "right", source: "gamepad" }]);
  result = stepLibraryGamepad(result.state, gamepadSnapshot({ axes: [0.8, 0] }), 1420);
  assert.deepEqual(result.actions, [{ type: "move", direction: "right", source: "gamepad" }]);

  result = stepLibraryGamepad(result.state, gamepadSnapshot({ axes: [0.2, 0] }), 1430);
  assert.deepEqual(result.actions, []);
  assert.equal(result.state.navigationDirection, null);
  assert.equal(result.state.nextNavigationRepeatAt, null);
});

test("gamepad confirm, back, context and paging fire on button edges only", () => {
  const pressed = gamepadSnapshot({ pressed: [0, 1, 2, 4, 5] });
  let result = stepLibraryGamepad(createLibraryGamepadState(), pressed, 1000);
  assert.deepEqual(result.actions, [
    { type: "page", direction: "left", source: "gamepad" },
    { type: "page", direction: "right", source: "gamepad" },
    { type: "confirm", source: "gamepad" },
    { type: "back", source: "gamepad" },
    { type: "context", source: "gamepad" }
  ]);

  result = stepLibraryGamepad(result.state, pressed, 1100);
  assert.deepEqual(result.actions, []);
  result = stepLibraryGamepad(result.state, gamepadSnapshot(), 1200);
  result = stepLibraryGamepad(result.state, gamepadSnapshot({ pressed: [0] }), 1300);
  assert.deepEqual(result.actions, [{ type: "confirm", source: "gamepad" }]);

  result = stepLibraryGamepad(result.state, gamepadSnapshot({ connected: false }), 1400);
  assert.deepEqual(result.actions, []);
  assert.deepEqual(result.state, createLibraryGamepadState());
});
