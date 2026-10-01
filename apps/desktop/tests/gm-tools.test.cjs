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
  getArkGmCreatureOptions,
  getArkGmItemOptions,
  getArkGmNumericItemOptions,
  buildArkCreatureOptions,
  buildGmToolCommand,
  getDstPrefabOptions,
  getGmToolCatalog,
  getInitialGmToolValues,
  localizeArkCatalogCategory,
  localizeArkCatalogName,
  localizeDstPrefabCategory,
  localizeDstPrefabName,
  searchArkGmCreatureOptions,
  searchArkGmItemOptions,
  searchArkGmNumericItemOptions,
  searchDstPrefabOptions,
  parseArkEnabledModIds,
  moduleHasGmTools
} = require(path.join(desktopRoot, "src", "views", "servers", "gm-tools.ts"));

test("ASA routes spawning to its dedicated integration and builds remote player item commands", () => {
  assert.equal(moduleHasGmTools("arksurvivalascended"), true);
  const catalog = getGmToolCatalog("arksurvivalascended");
  assert.equal(catalog?.title, "ARK GM Tools");

  const spawnTool = catalog.tools.find((tool) => tool.id === "ark_spawn_creature");
  assert.deepEqual(spawnTool.fields, []);
  const spawn = buildGmToolCommand("arksurvivalascended", "ark_spawn_creature", {
    creatureId: "Rex_Character_BP_C", forceTamed: "true", level: "150"
  });
  assert.deepEqual(spawn.commands, []);
  assert.match(spawn.error, /dedicated creature spawning panel/);

  const item = buildGmToolCommand("arksurvivalascended", "ark_give_item_to_player", {
    itemMode: "number",
    playerId: "123456789",
    itemId: "9",
    quantity: "50",
    quality: "1",
    blueprint: "0"
  });
  assert.deepEqual(item.commands, ["GiveItemNumToPlayer 123456789 9 50 1 0"]);

  const modItem = buildGmToolCommand("arksurvivalascended", "ark_give_item_to_player", {
    itemMode: "blueprint",
    playerId: "123456789",
    blueprintPath: "Blueprint'/Game/Mods/Example/PrimalItem_Test.PrimalItem_Test'",
    quantity: "2",
    quality: "0",
    blueprint: "0"
  });
  assert.deepEqual(modItem.commands, [
    "GiveItemToPlayer 123456789 \"Blueprint'/Game/Mods/Example/PrimalItem_Test.PrimalItem_Test'\" 2 0 0"
  ]);

  const batchItem = buildGmToolCommand("arksurvivalascended", "ark_give_item_to_player", {
    itemMode: "batch",
    playerId: "123456789",
    lines: "9,50,1,0\n76,1,2,0"
  });
  assert.deepEqual(batchItem.commands, [
    "GiveItemNumToPlayer 123456789 9 50 1 0",
    "GiveItemNumToPlayer 123456789 76 1 2 0"
  ]);
});

test("ARK GM tools use RCON-shaped command text for world and reward actions", () => {
  const clear = buildGmToolCommand("arksurvivalevolved", "ark_destroy_wild_dinos", {});
  assert.deepEqual(clear.commands, ["DestroyWildDinos"]);
  assert.equal(clear.dispatchOptions.transport, "source_rcon");
  assert.equal(clear.dispatchOptions.portName, "rcon");
  assert.equal(clear.dispatchOptions.passwordSettingKey, "admin_password");
  assert.equal(clear.dispatchOptions.enabledSettingKey, "rcon_enabled");

  const time = buildGmToolCommand("arksurvivalevolved", "ark_set_time", { time: "12:30" });
  assert.deepEqual(time.commands, ["SetTimeOfDay 12:30"]);

  assert.equal(
    [...clear.commands, ...time.commands].some((command) =>
      /^cheat\b/i.test(command)
    ),
    false
  );
});

test("ARK creature picker reads enabled mods and merges saved mod creature classes", () => {
  const asaSettingsJson = JSON.stringify({ mod_ids_csv: "927084, 929420,927084" });
  assert.deepEqual(parseArkEnabledModIds("arksurvivalascended", asaSettingsJson), ["927084", "929420"]);
  assert.deepEqual(parseArkEnabledModIds("arksurvivalevolved", JSON.stringify({ active_mod_ids: "839162288, 893735676" })), [
    "839162288",
    "893735676"
  ]);

  const customCreatures = [
    {
      label: "Modded Omega Rex",
      classId: "OmegaRex_Character_BP_C",
      modId: "927084"
    },
    {
      label: "Manual Dragon",
      classId: "ManualDragon_Character_BP_C"
    }
  ];
  const options = buildArkCreatureOptions("arksurvivalascended", asaSettingsJson, customCreatures);
  assert.ok(options.some((option) => option.value === "Rex_Character_BP_C" && /Official/.test(option.label)));
  assert.ok(options.some((option) => option.value === "OmegaRex_Character_BP_C" && /927084/.test(option.label)));
  assert.ok(options.some((option) => option.value === "ManualDragon_Character_BP_C" && /Saved/.test(option.label)));

  const catalog = getGmToolCatalog("arksurvivalascended");
  assert.deepEqual(catalog.tools.find((tool) => tool.id === "ark_spawn_creature").fields, []);
});

test("ARK GM tools merge equivalent spawn and item actions into compact forms", () => {
  const catalog = getGmToolCatalog("arksurvivalascended");
  const spawnCreature = catalog?.tools.find((tool) => tool.id === "ark_spawn_creature");
  const giveItem = catalog?.tools.find((tool) => tool.id === "ark_give_item_to_player");
  const zhMessages = fs.readFileSync(path.join(desktopRoot, "src", "i18n-messages-zh-extra.ts"), "utf8");
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "GMToolsWorkbench.tsx"), "utf8");
  const css = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "workbench", "operations", "gm-tools.css"), "utf8");

  assert.equal(catalog?.tools.some((tool) => tool.id === "ark_spawn_tamed_dino"), false);
  assert.equal(catalog?.tools.some((tool) => tool.id === "ark_spawn_wild_creature"), false);
  assert.equal(catalog?.tools.some((tool) => tool.id === "ark_give_item_num_to_player"), false);
  assert.equal(catalog?.tools.some((tool) => tool.id === "ark_give_item_blueprint_to_player"), false);
  assert.equal(catalog?.tools.some((tool) => tool.id === "ark_give_item_num_lines_to_player"), false);
  assert.deepEqual(spawnCreature?.fields, []);
  assert.equal(giveItem?.fields.find((field) => field.key === "itemMode")?.type, "select");
  assert.equal(zhMessages.includes("Class ID"), false);
  assert.deepEqual(giveItem?.fields.find((field) => field.key === "blueprintPath")?.visibleWhen, {
    field: "itemMode",
    values: ["blueprint"]
  });
  assert.match(source, /isFieldVisible\(field, toolValues\)/);
  assert.match(source, /<ArkCreatureSpawner/);
  assert.match(source, /instanceId=\{props\.details\.summary\.id\}/);
  assert.match(source, /settingsJson=\{props\.details\.settings_json\}/);
  assert.doesNotMatch(source, /navigator\.clipboard|client_console/);
  assert.match(css, /\.gmt-fields-grid--ark-give-item/);
});

test("ARK GM pickers expose broad built-in creature and player item catalogs", () => {
  const itemOptions = getArkGmItemOptions();
  const numericItemOptions = getArkGmNumericItemOptions();
  const creatureOptions = getArkGmCreatureOptions();

  assert.ok(itemOptions.length >= 1000, `expected a broad ARK item catalog, got ${itemOptions.length}`);
  assert.ok(creatureOptions.length >= 1000, `expected a broad ARK creature catalog, got ${creatureOptions.length}`);
  assert.ok(numericItemOptions.length >= 450, `expected ARK numeric item entries, got ${numericItemOptions.length}`);
  assert.ok(itemOptions.some((option) => option.name === "Element" && /PrimalItemResource_Element/.test(option.blueprintPath)));
  assert.ok(itemOptions.some((option) => option.name === "Simple Pistol" && /PrimalItem_WeaponGun/.test(option.blueprintPath)));
  assert.ok(creatureOptions.some((option) => option.value === "Rex_Character_BP_C"));
  assert.equal(searchArkGmItemOptions("stone", 20)[0]?.name, "Stone");
  assert.ok(searchArkGmItemOptions("chitin", 20).some((option) => option.name === "Chitin"));
  assert.ok(searchArkGmNumericItemOptions("stone", 20).some((option) => option.name === "Stone" && option.itemId === "8"));
  assert.equal(searchArkGmCreatureOptions("rex", 30)[0]?.value, "Rex_Character_BP_C");
  assert.ok(searchArkGmCreatureOptions("rex", 30).some((option) => option.value === "Rex_Character_BP_C"));

  const catalog = getGmToolCatalog("arksurvivalascended");
  const spawnCreature = catalog?.tools.find((tool) => tool.id === "ark_spawn_creature");
  const giveItem = catalog?.tools.find((tool) => tool.id === "ark_give_item_to_player");
  assert.deepEqual(spawnCreature?.fields, []);
  assert.equal(giveItem?.fields.find((field) => field.key === "itemMode")?.defaultValue, "blueprint");
  assert.ok((giveItem?.fields.find((field) => field.key === "blueprintPath")?.options?.length ?? 0) >= 1000);
  assert.ok((giveItem?.fields.find((field) => field.key === "itemId")?.options?.length ?? 0) >= 450);
});

test("ARK GM workbench uses compact paginated pickers for large built-in catalogs", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "GMToolsWorkbench.tsx"), "utf8");
  const css = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "workbench", "operations", "gm-tools.css"), "utf8");

  assert.match(source, /ArkCreatureSpawner/);
  assert.match(source, /searchArkGmItemOptions/);
  assert.match(source, /isArkCatalogField/);
  assert.match(source, /gmt-ark-catalog-assist/);
  assert.match(source, /ARK_CATALOG_SEARCH_RESULT_LIMIT = 12/);
  assert.match(source, /!isArkCatalogField\(moduleId, field\)/);
  assert.match(css, /\.gmt-ark-catalog-assist/);
  assert.match(css, /\.gmt-ark-catalog-page-btn/);
});

test("GM large catalogs localize item and creature display names without changing command values", () => {
  assert.equal(typeof localizeArkCatalogName, "function");
  assert.equal(typeof localizeDstPrefabName, "function");

  const stone = getArkGmItemOptions().find((option) => option.name === "Stone");
  const tusoteuthisOil = getArkGmItemOptions().find((option) => option.name === "Oil (Tusoteuthis)");
  const absorbentSubstrate = getArkGmItemOptions().find((option) => option.name === "Absorbent Substrate");
  const rex = getArkGmCreatureOptions().find((option) => option.value === "Rex_Character_BP_C");
  const dungBeetle = getArkGmCreatureOptions().find((option) => option.name === "Aberrant Dung Beetle");
  const log = getDstPrefabOptions().find((option) => option.value === "log");
  const cane = getDstPrefabOptions().find((option) => option.value === "cane");
  const waxedAsparagus = getDstPrefabOptions().find((option) => option.value === "asparagus_oversized_waxed");

  assert.ok(stone);
  assert.ok(tusoteuthisOil);
  assert.ok(absorbentSubstrate);
  assert.ok(rex);
  assert.ok(dungBeetle);
  assert.ok(log);
  assert.ok(cane);
  assert.ok(waxedAsparagus);
  assert.equal(localizeArkCatalogName(stone, "zh-CN"), "石头");
  assert.equal(localizeArkCatalogName(absorbentSubstrate, "zh-CN"), "吸附基质");
  assert.equal(localizeArkCatalogName(tusoteuthisOil, "zh-CN"), "油（托斯特巨鱿）");
  assert.equal(localizeArkCatalogCategory(stone, "zh-CN"), "资源");
  assert.equal(localizeArkCatalogName(rex, "zh-CN"), "霸王龙");
  assert.equal(localizeArkCatalogName(dungBeetle, "zh-CN"), "畸变粪甲虫");
  assert.equal(localizeArkCatalogCategory(rex, "zh-CN"), "官方");
  assert.equal(localizeArkCatalogName(stone, "en-US"), "Stone");
  assert.equal(localizeArkCatalogCategory(stone, "en-US"), "Resource");
  assert.equal(localizeDstPrefabName(log, "zh-CN"), "木头");
  assert.equal(localizeDstPrefabCategory(log, "zh-CN"), "材料");
  assert.equal(localizeDstPrefabName(log, "en-US"), "Logs");
  assert.equal(localizeDstPrefabCategory(log, "en-US"), "Materials");
  assert.equal(localizeDstPrefabName(cane, "en-US"), "Walking Cane");
  assert.equal(localizeDstPrefabName(waxedAsparagus, "zh-CN"), "芦笋巨型上蜡");

  assert.equal(searchArkGmItemOptions("石头", 5)[0]?.name, "Stone");
  assert.equal(searchArkGmCreatureOptions("霸王龙", 5)[0]?.value, "Rex_Character_BP_C");
  assert.equal(searchDstPrefabOptions("芦笋巨型上蜡", 5)[0]?.value, "asparagus_oversized_waxed");
  assert.equal(searchDstPrefabOptions("walking cane", 5)[0]?.value, "cane");
  assert.ok(searchDstPrefabOptions("materials", 50).some((option) => option.value === "log"));
});

test("GM large catalog localization covers every built-in display name", () => {
  const asciiWord = /[A-Za-z]{3,}/;
  const hanText = /[\u3400-\u9fff]/;

  for (const option of getArkGmItemOptions()) {
    assert.equal(asciiWord.test(localizeArkCatalogName(option, "zh-CN")), false, `ARK item still has English display text: ${option.name}`);
    assert.equal(asciiWord.test(localizeArkCatalogCategory(option, "zh-CN")), false, `ARK item category still has English text: ${option.category}`);
  }

  for (const option of getArkGmCreatureOptions()) {
    assert.equal(asciiWord.test(localizeArkCatalogName(option, "zh-CN")), false, `ARK creature still has English display text: ${option.name}`);
    assert.equal(asciiWord.test(localizeArkCatalogCategory(option, "zh-CN")), false, `ARK creature category still has English text: ${option.category}`);
  }

  for (const option of getDstPrefabOptions()) {
    assert.equal(asciiWord.test(localizeDstPrefabName(option, "zh-CN")), false, `DST prefab still has English display text: ${option.value}`);
    assert.equal(asciiWord.test(localizeDstPrefabCategory(option, "zh-CN")), false, `DST category still has English text: ${option.category}`);
    assert.equal(hanText.test(localizeDstPrefabName(option, "en-US")), false, `DST English name still has Chinese text: ${option.value}`);
    assert.equal(hanText.test(localizeDstPrefabCategory(option, "en-US")), false, `DST English category still has Chinese text: ${option.category}`);
  }
});

test("GM command definitions do not carry removed actions", () => {
  const sources = [
    path.join(desktopRoot, "src", "views", "servers", "gm-tools.ts"),
    path.join(desktopRoot, "src", "i18n-messages-en-extra.ts"),
    path.join(desktopRoot, "src", "i18n-messages-zh-extra.ts")
  ].map((filePath) => [filePath, fs.readFileSync(filePath, "utf8")]);
  const removedIds = [
    "servers.gmTools.catalog.rust",
    "servers.gmTools.catalog.sevendaystodie",
    "servers.gmTools.catalog.vrising",
    "servers.gmTools.options.zomboidRole",
    "palworld_show_players",
    "palworld_kick_player",
    "palworld_ban_player",
    "palworld_unban_player",
    "sevendays_list_players",
    "sevendays_kick_player",
    "sevendays_ban_player",
    "sevendays_unban_player",
    "terraria_list_players",
    "terraria_kick_player",
    "terraria_ban_player",
    "minecraft_list_players",
    "minecraft_kick_player",
    "minecraft_ban_player",
    "minecraft_pardon_player",
    "minecraft_op_player",
    "minecraft_deop_player",
    "minecraft_whitelist_list",
    "minecraft_whitelist_add",
    "minecraft_whitelist_remove",
    "zomboid_list_players",
    "zomboid_kick_user",
    "zomboid_ban_user",
    "zomboid_ban_steamid",
    "zomboid_unban_user",
    "zomboid_set_access_level",
    "zomboid_add_to_whitelist",
    "zomboid_remove_from_whitelist",
    "vrising_list_users",
    "vrising_reload_banlist",
    "vrising_kick_user",
    "vrising_ban_user",
    "vrising_unban_user",
    "rust_status",
    "rust_players",
    "rust_users",
    "rust_ban_list",
    "rust_write_cfg",
    "rust_kick_player",
    "rust_ban_player",
    "rust_unban_player",
    "rust_add_owner",
    "rust_remove_owner",
    "rust_add_moderator",
    "rust_remove_moderator",
    "ark_spawn_tamed_dino",
    "ark_spawn_wild_creature",
    "ark_give_item_num_to_player",
    "ark_give_item_blueprint_to_player",
    "ark_give_item_num_lines_to_player",
    "dst_reward_all_players",
    "dst_spawn_prefab",
    "sevendays_save_world"
  ];

  for (const [filePath, source] of sources) {
    for (const removedId of removedIds) {
      assert.equal(source.includes(removedId), false, `${removedId} must not live in GM tools resources: ${filePath}`);
    }
  }

  assert.equal(moduleHasGmTools("rust"), false);
  assert.equal(moduleHasGmTools("vrising"), false);
  assert.equal(moduleHasGmTools("sevendaystodie"), false);
});

test("DST GM tools render one shard-aware player item tool with inventory placement toggle", () => {
  assert.equal(moduleHasGmTools("dontstarve"), true);

  const spawn = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
    shard: "caves",
    playerIndex: "2",
    allPlayers: "false",
    placeInInventory: "false",
    prefab: "spider",
    amount: "3"
  });
  assert.match(spawn.commands[0], /local p=AllPlayers\[2\]/);
  assert.match(spawn.commands[0], /for i=1,3 do local e=assert\(SpawnPrefab\("spider"\)/);
  assert.match(spawn.commands[0], /e.Transform:SetPosition\(x,y,z\)/);
  assert.equal(spawn.processKey, "caves");
  assert.equal(spawn.dispatchOptions.transport, "stdin");

  const singleReward = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
    shard: "master",
    playerIndex: "2",
    allPlayers: "false",
    placeInInventory: "true",
    prefab: "log",
    amount: "20"
  });
  assert.equal(singleReward.processKey, "master");
  assert.match(singleReward.commands[0], /local p=AllPlayers\[2\]/);
  assert.match(singleReward.commands[0], /p.components.inventory:GiveItem/);
  assert.doesNotMatch(singleReward.commands[0], /c_give/);

  const reward = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
    shard: "master",
    playerIndex: "",
    allPlayers: "true",
    placeInInventory: "true",
    prefab: "log",
    amount: "20"
  });
  assert.equal(reward.processKey, "master");
  assert.match(reward.commands[0], /ipairs\(AllPlayers\)/);
  assert.match(reward.commands[0], /for i=1,20 do local e=assert\(SpawnPrefab\("log"\)/);
  assert.match(reward.commands[0], /p.components.inventory:GiveItem/);

  const spawnForAllPlayers = buildGmToolCommand("dontstarve", "dst_give_item_to_player", {
    shard: "master",
    playerIndex: "",
    allPlayers: "true",
    placeInInventory: "false",
    prefab: "spider",
    amount: "2"
  });
  assert.equal(spawnForAllPlayers.processKey, "master");
  assert.match(spawnForAllPlayers.commands[0], /ipairs\(AllPlayers\)/);
  assert.match(spawnForAllPlayers.commands[0], /for i=1,2 do local e=assert\(SpawnPrefab\("spider"\)/);
  assert.match(spawnForAllPlayers.commands[0], /e.Transform:SetPosition\(x,y,z\)/);
});

test("DST GM item picker exposes a broad categorized prefab catalog", () => {
  const options = getDstPrefabOptions();
  assert.ok(options.length >= 500, `expected a broad DST prefab catalog, got ${options.length}`);
  assert.ok(options.some((option) => option.value === "cane" && /装备/.test(option.label)));
  assert.ok(options.some((option) => option.value === "bonestew" && /烹饪/.test(option.label)));
  assert.ok(options.some((option) => option.value === "opalpreciousgem" && /魔法/.test(option.label)));

  const catalog = getGmToolCatalog("dontstarve");
  assert.equal(catalog?.tools.some((tool) => tool.id === "dst_reward_all_players"), false);
  assert.equal(catalog?.tools.some((tool) => tool.id === "dst_spawn_prefab"), false);
  const giveItem = catalog?.tools.find((tool) => tool.id === "dst_give_item_to_player");
  assert.equal(giveItem?.fields.find((field) => field.key === "shard")?.type, "select");
  assert.equal(giveItem?.fields.find((field) => field.key === "allPlayers")?.type, "checkbox");
  assert.equal(giveItem?.fields.find((field) => field.key === "placeInInventory")?.type, "checkbox");
  assert.equal(giveItem?.fields.find((field) => field.key === "placeInInventory")?.defaultValue, "true");
  const prefabField = giveItem?.fields.find((field) => field.key === "prefab");
  assert.ok((prefabField?.options?.length ?? 0) >= 500);
});

test("DST GM item picker search matches Chinese names, categories, and prefab codes", () => {
  assert.equal(searchDstPrefabOptions("步行手杖", 10)[0]?.value, "cane");
  assert.ok(searchDstPrefabOptions("烹饪", 30).some((option) => option.value === "bonestew"));
  assert.ok(searchDstPrefabOptions("gold", 30).some((option) => option.value === "goldnugget"));

  for (const option of getDstPrefabOptions()) {
    assert.match(option.value, /^[A-Za-z0-9_./-]+$/);
    assert.equal(/[\r\n\t]/.test(option.label), false);
  }
});

test("GM workbench renders a dedicated DST prefab picker assist for the large catalog", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "GMToolsWorkbench.tsx"), "utf8");
  assert.match(source, /searchDstPrefabOptions/);
  assert.match(source, /isDstPrefabField/);
  assert.match(source, /gmt-dst-prefab-assist/);
});

test("DST GM prefab picker uses a compact custom chooser instead of a giant native dropdown", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "GMToolsWorkbench.tsx"), "utf8");
  const css = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "workbench", "operations", "gm-tools.css"), "utf8");

  assert.match(source, /function shouldUseFieldDatalist/);
  assert.match(source, /!isDstPrefabField\(moduleId, field\)/);
  assert.match(source, /DST_PREFAB_SEARCH_RESULT_LIMIT = 12/);
  assert.match(source, /gmt-field--dst-prefab/);
  assert.match(css, /\.gmt-field--dst-prefab/);
  assert.match(css, /grid-template-columns:\s*repeat\(auto-fit, minmax\(180px, 1fr\)\)/);
  assert.match(css, /max-height:\s*146px/);
  assert.doesNotMatch(css, /\.gmt-dst-prefab-option\s*\{[\s\S]*?min-height:\s*58px/);
});

test("DST GM item form keeps target and quantity on one compact row", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "GMToolsWorkbench.tsx"), "utf8");
  const css = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "workbench", "operations", "gm-tools.css"), "utf8");
  const catalog = getGmToolCatalog("dontstarve");
  const giveTool = catalog?.tools.find((tool) => tool.id === "dst_give_item_to_player");
  const fieldKeys = giveTool?.fields.map((field) => field.key);

  assert.deepEqual(fieldKeys?.slice(0, 3), ["shard", "playerIndex", "amount"]);
  assert.match(source, /fieldGridClassName\(moduleId, activeTool\)/);
  assert.match(css, /\.gmt-fields-grid--dst-item-action\s*\{[\s\S]*?grid-template-columns:\s*minmax\(180px, 1fr\) minmax\(130px, 160px\) minmax\(110px, 140px\)/);
});

test("GM workbench keeps command generation internal and shows the full DST prefab catalog size", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "GMToolsWorkbench.tsx"), "utf8");
  const css = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "workbench", "operations", "gm-tools.css"), "utf8");

  assert.match(source, /getDstPrefabOptions/);
  assert.match(source, /DST_PREFAB_TOTAL_COUNT/);
  assert.match(source, /total:\s*DST_PREFAB_TOTAL_COUNT/);
  assert.doesNotMatch(source, /className="gmt-preview"/);
  assert.doesNotMatch(source, /gmt-preview-code/);
  assert.doesNotMatch(source, /gmt-cmd-count-badge/);
  assert.doesNotMatch(css, /\.gmt-preview/);
  assert.doesNotMatch(css, /\.gmt-cmd-count-badge/);
});

test("DST GM prefab picker paginates broad result sets with compact chevron controls", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "GMToolsWorkbench.tsx"), "utf8");
  const css = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "workbench", "operations", "gm-tools.css"), "utf8");

  assert.match(source, /const \[pageIndex, setPageIndex\] = useState\(0\)/);
  assert.match(source, /const allResults = useMemo/);
  assert.match(source, /Math\.ceil\(allResults\.length \/ DST_PREFAB_SEARCH_RESULT_LIMIT\)/);
  assert.match(source, /allResults\.slice\(pageStart, pageStart \+ DST_PREFAB_SEARCH_RESULT_LIMIT\)/);
  assert.match(source, /gmt-dst-prefab-pager/);
  assert.match(source, /chevron-left/);
  assert.match(source, /chevron-right/);
  assert.match(css, /\.gmt-dst-prefab-pager/);
  assert.match(css, /\.gmt-dst-prefab-page-btn/);
});

test("DST GM prefab selection keeps the browsed result list open", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "GMToolsWorkbench.tsx"), "utf8");

  assert.match(source, /const activeQuery = props\.query\.trim\(\);/);
  assert.doesNotMatch(source, /props\.query\.trim\(\) \|\| props\.value\.trim\(\)/);
  assert.doesNotMatch(source, /setDstPrefabSearchByField\(\(current\) => \(\{ \.\.\.current, \[key\]: "" \}\)\)/);
});

test("DST GM visible copy avoids raw prefab jargon", () => {
  const zhMessages = fs.readFileSync(path.join(desktopRoot, "src", "i18n-messages-zh-extra.ts"), "utf8");
  const catalog = getGmToolCatalog("dontstarve");
  const giveTool = catalog?.tools.find((tool) => tool.id === "dst_give_item_to_player");

  assert.equal(zhMessages.includes("刷 prefab"), false);
  assert.equal(zhMessages.includes('"Prefab"'), false);
  assert.equal(catalog?.tools.some((tool) => tool.id === "dst_spawn_prefab"), false);
  assert.equal(giveTool?.title, "Give or spawn item");
  assert.equal(zhMessages.includes("物品/实体 ID"), false);
  assert.equal(giveTool?.fields.find((field) => field.key === "prefab")?.label, "Item / Creature");
  assert.equal(giveTool?.fields.find((field) => field.key === "placeInInventory")?.label, "Put in inventory");
});

test("palworld maintenance actions do not create a duplicate tools tab", () => {
  assert.equal(moduleHasGmTools("palworld"), false);
  assert.equal(getGmToolCatalog("palworld"), null);
  for (const toolId of ["palworld_save_world", "palworld_broadcast"]) {
    const result = buildGmToolCommand("palworld", toolId, {});
    assert.deepEqual(result.commands, []);
    assert.match(result.error, /does not have GM tools/);
  }
  const moduleSource = fs.readFileSync(path.join(root, "modules", "palworld", "module.toml"), "utf8");
  assert.match(moduleSource, /id = "save_world"/);
  assert.match(moduleSource, /id = "broadcast"/);
});

test("7 Days to Die does not expose saveworld as a GM command", () => {
  assert.equal(moduleHasGmTools("sevendaystodie"), false);
  const catalog = getGmToolCatalog("sevendaystodie");
  assert.equal(catalog, null);

  const save = buildGmToolCommand("sevendaystodie", "sevendays_save_world", {});
  assert.deepEqual(save.commands, []);
  assert.match(save.error ?? "", /does not have GM tools/);
});

test("terraria maintenance actions do not create a duplicate tools tab", () => {
  assert.equal(moduleHasGmTools("terraria"), false);
  assert.equal(getGmToolCatalog("terraria"), null);
  for (const toolId of ["terraria_save_world", "terraria_broadcast"]) {
    const result = buildGmToolCommand("terraria", toolId, {});
    assert.deepEqual(result.commands, []);
    assert.match(result.error, /does not have GM tools/);
  }
  const moduleSource = fs.readFileSync(path.join(root, "modules", "terraria", "module.toml"), "utf8");
  assert.match(moduleSource, /id = "save_world"/);
  assert.match(moduleSource, /id = "broadcast"/);
});

test("minecraft maintenance actions do not create a duplicate tools tab", () => {
  assert.equal(moduleHasGmTools("minecraft"), false);
  assert.equal(getGmToolCatalog("minecraft"), null);
  for (const toolId of ["minecraft_save_world"]) {
    const result = buildGmToolCommand("minecraft", toolId, {});
    assert.deepEqual(result.commands, []);
    assert.match(result.error, /does not have GM tools/);
  }
  const moduleSource = fs.readFileSync(path.join(root, "modules", "minecraft", "module.toml"), "utf8");
  assert.match(moduleSource, /id = "save_world"/);
  assert.match(moduleSource, /id = "broadcast"/);
});

test("projectzomboid maintenance actions do not create a duplicate tools tab", () => {
  assert.equal(moduleHasGmTools("projectzomboid"), false);
  assert.equal(getGmToolCatalog("projectzomboid"), null);
  for (const toolId of ["zomboid_save_world"]) {
    const result = buildGmToolCommand("projectzomboid", toolId, {});
    assert.deepEqual(result.commands, []);
    assert.match(result.error, /does not have GM tools/);
  }
  const moduleSource = fs.readFileSync(path.join(root, "modules", "projectzomboid", "module.toml"), "utf8");
  assert.match(moduleSource, /id = "save_world"/);
  assert.match(moduleSource, /id = "broadcast"/);
});

test("every generic tool builds a command and the dedicated spawn entry rejects generic dispatch", () => {
  for (const moduleId of ["arksurvivalascended", "arksurvivalevolved", "dontstarve"]) {
    const catalog = getGmToolCatalog(moduleId);
    assert.ok(catalog, `${moduleId} catalog missing`);
    for (const tool of catalog.tools) {
      const values = getInitialGmToolValues(tool);
      if (tool.id === "ark_give_item_to_player") {
        values.playerId = "123456789";
        values.itemId = "9";
      }
      const result = buildGmToolCommand(moduleId, tool.id, values);
      if (tool.id === "ark_spawn_creature") {
        assert.deepEqual(result.commands, []);
        assert.match(result.error, /dedicated creature spawning panel/);
        continue;
      }
      assert.equal(result.error, null, `${moduleId}:${tool.id} should not error`);
      assert.ok(result.commands.length > 0, `${moduleId}:${tool.id} should build commands`);
      assert.ok(result.preview.length > 0, `${moduleId}:${tool.id} should build preview`);
    }
  }
});

test("GM command builder reports missing required values instead of producing partial commands", () => {
  const result = buildGmToolCommand("arksurvivalascended", "ark_give_item_to_player", {
    itemMode: "number", playerId: "", itemId: "9", quantity: "1", quality: "0", blueprint: "0"
  });
  assert.deepEqual(result.commands, []);
  assert.match(result.error ?? "", /Player ID/);
  assert.equal(moduleHasGmTools("valheim"), false);
});
