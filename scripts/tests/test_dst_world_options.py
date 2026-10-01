from __future__ import annotations

import copy
import contextlib
import importlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
import zipfile

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))


def sample_inventory() -> dict:
    return {
        "gameVersion": "123456",
        "scriptsSha256": "a" * 64,
        "groups": {
            "worldgen": [{"id": "misc", "order": 1, "textKey": "STRINGS.UI.MISC"}],
            "settings": [{"id": "global", "order": 0, "textKey": "STRINGS.UI.GLOBAL"}],
        },
        "options": [
            {"key": "trees", "category": "worldgen", "group": "misc", "order": None,
             "locations": ["forest", "cave"], "masterControlled": False,
             "values": {"forest": ["never", "default", "often"], "cave": ["never", "default", "often"]}},
            {"key": "day", "category": "settings", "group": "global", "order": 1,
             "locations": ["forest", "cave"], "masterControlled": True,
             "values": {"forest": ["default", "long"], "cave": ["default", "long"]}},
        ],
    }


def sample_schema() -> dict:
    properties = {}
    for field, section, key, values in [
        ("master_trees", "mastergen", "trees", ["never", "default", "often"]),
        ("caves_trees", "cavesgen", "trees", ["never", "default", "often"]),
        ("master_day", "mastersettings", "day", ["default", "long"]),
    ]:
        properties[field] = {"x-lsgm-section": section, "x-lsgm-source-key": "overrides." + key, "enum": values}
    return {"properties": properties}


def write_package(root: Path, *, mutate=None) -> None:
    """Synthetic Lua fixtures: no proprietary scripts are copied into tests."""
    customize = '''
local frequency_descriptions
local worldgen_frequency_descriptions
if IsNotConsole() then
 frequency_descriptions = {{text=STRINGS.A,data="never"},{text=STRINGS.B,data="default"},{text=STRINGS.C,data="often"}}
 worldgen_frequency_descriptions = {{text=STRINGS.A,data="never"},{text=STRINGS.B,data="default"},{text=STRINGS.C,data="insane"}}
else
 frequency_descriptions = {{text=STRINGS.B,data="default"}}
 worldgen_frequency_descriptions = {{text=STRINGS.B,data="default"}}
end
local size_descriptions = nil
if IsPS4() then
 size_descriptions = {{text=STRINGS.B,data="default"}}
else
 size_descriptions = {{text=STRINGS.A,data="small"},{text=STRINGS.B,data="default"},{text=STRINGS.C,data="huge"}}
end
local ocean_worldgen_frequency_descriptions = {}
for i, data in ipairs(worldgen_frequency_descriptions) do
 ocean_worldgen_frequency_descriptions[i] = {text=data.text,data="ocean_"..data.data}
end
local WORLDGEN_GROUP = {
 ["misc"] = {order=1,text=STRINGS.UI.MISC,desc=worldgen_frequency_descriptions,atlas="ui.xml",items={
  ["trees"] = {value="default",image="trees.tex",world={"forest","cave"}},
  ["task_set"] = {value="default",image="map.tex",desc=tasksets.GetGenTaskLists},
  ["start_location"] = {value="default",image="map.tex",desc=startlocations.GetGenStartLocations},
  ["world_size"] = {value="default",image="map.tex",desc=size_descriptions,order=3},
  ["ocean_kelp"] = {value="ocean_default",image="kelp.tex",desc=ocean_worldgen_frequency_descriptions,world={"forest"}},
 }}
}
local WORLDSETTINGS_GROUP = {
 ["global"] = {order=0,text=STRINGS.UI.GLOBAL,desc=frequency_descriptions,atlas="ui.xml",items={
  ["day"] = {value="default",image="day.tex",master_controlled=true,order=1}
 }}
}
local MOD_WORLDGEN_GROUP = {}
local MOD_WORLDSETTINGS_GROUP = {}
local function options(location,is_master_world,item)
 if location == nil or item.world == nil or table.contains(item.world, location) then
  if is_master_world or not item.master_controlled then return item end
 end
end
local function reset() MOD_WORLDGEN_GROUP = {} MOD_WORLDSETTINGS_GROUP = {} end
'''
    sources = {
        "scripts/map/customize.lua": customize,
        "scripts/map/tasksets.lua": '''
local function options(v,world)
 if not v.hideinfrontend and world == nil or v.location == world then return v end
end
require("map/tasksets/forest")
require("map/tasksets/caves")
require("map/tasksets/lavaarena")
''',
        "scripts/map/tasksets/forest.lua": 'AddTaskSet("default", {location="forest"})\nAddTaskSet("classic", {location="forest"})',
        "scripts/map/tasksets/caves.lua": 'local taskset_data = {location="cave"}\nAddTaskSet("cave_default", taskset_data)',
        "scripts/map/tasksets/lavaarena.lua": 'AddTaskSet("arena", {location="lavaarena"})',
        "scripts/map/startlocations.lua": '''
local function options(v,world) if world == nil or v.location == world then return v end end
function AddStartLocation(name,data) end
AddStartLocation("default", {location="forest"})
AddStartLocation("darkness", {location="forest"})
AddStartLocation("caves", {location="cave"})
''',
    }
    if mutate is not None:
        mutate(sources)
    (root / "version.txt").write_text("123456\n", encoding="utf-8")
    archive = root / "data/databundles/scripts.zip"
    archive.parent.mkdir(parents=True)
    with zipfile.ZipFile(archive, "w") as bundle:
        for name, content in sources.items():
            bundle.writestr(name, content)


class WorldOptionsTests(unittest.TestCase):
    def setUp(self) -> None:
        self.assertIsNotNone(importlib.util.find_spec("dst_world_options"), "DST native inventory verifier is not implemented")
        self.module = importlib.import_module("dst_world_options")

    def test_schema_reports_missing_field_reduced_enum_and_wrong_page(self) -> None:
        schema = sample_schema()
        del schema["properties"]["caves_trees"]
        schema["properties"]["master_trees"]["enum"].remove("often")
        schema["properties"]["master_day"]["x-lsgm-section"] = "mastergen"
        errors = "\n".join(self.module.compare_schema(sample_inventory(), schema))
        self.assertIn("cavesgen: missing trees", errors)
        self.assertIn("mastergen.trees: enum", errors)
        self.assertIn("mastersettings: missing day", errors)
        self.assertIn("mastergen: unexpected day", errors)

    def test_master_controlled_values_are_only_editable_on_master(self) -> None:
        self.assertEqual(self.module.compare_schema(sample_inventory(), sample_schema()), [])

    def test_schema_reports_duplicate_native_mapping(self) -> None:
        schema = sample_schema()
        schema["properties"]["another_tree"] = copy.deepcopy(schema["properties"]["master_trees"])
        self.assertIn("duplicate trees", "\n".join(self.module.compare_schema(sample_inventory(), schema)))

    def test_native_inventory_detects_option_and_group_order_drift(self) -> None:
        tracked = sample_inventory()
        actual = copy.deepcopy(tracked)
        actual["options"][0]["order"] = 4
        actual["groups"]["worldgen"][0]["order"] = 7
        errors = "\n".join(self.module.compare_inventory(tracked, actual))
        self.assertIn("trees.order", errors)
        self.assertIn("worldgen.misc.order", errors)

    def test_native_inventory_detects_group_and_enum_drift(self) -> None:
        tracked = sample_inventory()
        actual = copy.deepcopy(tracked)
        actual["options"][0]["group"] = "animals"
        actual["options"][1]["values"]["forest"].append("short")
        errors = "\n".join(self.module.compare_inventory(tracked, actual))
        self.assertIn("trees.group", errors)
        self.assertIn("day.values", errors)

    def test_native_inventory_detects_new_and_removed_keys(self) -> None:
        tracked = sample_inventory()
        actual = copy.deepcopy(tracked)
        actual["options"][0]["key"] = "rocks"
        errors = "\n".join(self.module.compare_inventory(tracked, actual))
        self.assertIn("missing trees", errors)
        self.assertIn("new rocks", errors)

    def test_table_parser_handles_nested_multiline_data_and_comments(self) -> None:
        value = self.module.parse_table('''{
            -- ["ignored"] = { value = "never" },
            ["trees"] = { value = "default", order = 2,
                world = {"forest", "cave"}, label = "literal--text" },
            --[=[ { junk } ]=]
        }''')
        self.assertEqual(list(value), ["trees"])
        self.assertEqual(value["trees"]["world"], {1: "forest", 2: "cave"})
        self.assertEqual(value["trees"]["label"], "literal--text")

    def test_table_parser_rejects_executable_and_unrecognized_expressions(self) -> None:
        for source in ('{value = os.execute("echo bad")}', '{value = fn()}', '{value = "a" .. "b"}', '{value = 1 + 2}'):
            with self.subTest(source=source), self.assertRaisesRegex(ValueError, "Unsupported|Unexpected"):
                self.module.parse_table(source)

    def test_local_mod_declaration_is_separate_from_later_runtime_reset(self) -> None:
        lua = importlib.import_module("dst_world_options_lua")
        self.assertEqual(lua.assigned_table("local MOD_GROUP = {}\nfunction reset() MOD_GROUP = {} end", "MOD_GROUP", local_only=True), {})

    def test_inventory_writer_preserves_one_option_per_line_and_omits_paths(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "world-options.json"
            self.module.write_inventory(target, sample_inventory())
            output = target.read_text(encoding="utf-8")
            self.assertEqual(json.loads(output), sample_inventory())
            self.assertNotIn(directory, output)
            self.assertEqual(sum('"key":' in line for line in output.splitlines()), 2)

    def test_invalid_inventory_cannot_silently_drop_a_world(self) -> None:
        inventory = sample_inventory()
        del inventory["options"][0]["values"]["cave"]
        with self.assertRaisesRegex(ValueError, "trees.*values"):
            self.module.validate_inventory(inventory)

    def test_package_with_unsupported_lua_fails_without_executing_it(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "version.txt").write_text("123456\n", encoding="utf-8")
            archive = root / "data/databundles/scripts.zip"
            archive.parent.mkdir(parents=True)
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("scripts/map/customize.lua", 'local WORLDGEN_GROUP = build_options()')
            with self.assertRaisesRegex(ValueError, "Unsupported|Missing"):
                self.module.extract_inventory(root)

    def test_package_uses_pc_branches_group_fallback_and_ocean_values(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_package(root)
            inventory = self.module.extract_inventory(root)
        self.module.validate_inventory(inventory)
        options = {option["key"]: option for option in inventory["options"]}
        self.assertEqual(len(options), 6)
        self.assertEqual(options["trees"]["values"]["forest"], ["never", "default", "insane"])
        self.assertEqual(options["world_size"]["values"]["cave"], ["small", "default", "huge"])
        self.assertEqual(options["ocean_kelp"]["values"], {"forest": ["ocean_never", "ocean_default", "ocean_insane"]})
        self.assertEqual(options["day"]["values"]["forest"], ["never", "default", "often"])
        self.assertTrue(options["day"]["masterControlled"])
        self.assertEqual(inventory["groups"]["worldgen"], [{"id": "misc", "order": 1, "textKey": "STRINGS.UI.MISC"}])

    def test_package_resolves_function_enums_from_every_loaded_registration(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_package(root)
            options = {option["key"]: option for option in self.module.extract_inventory(root)["options"]}
        self.assertEqual(options["task_set"]["values"], {"forest": ["classic", "default"], "cave": ["cave_default"]})
        self.assertEqual(options["start_location"]["values"], {"forest": ["darkness", "default"], "cave": ["caves"]})

    def test_new_customization_properties_fail_instead_of_ignoring_visibility(self) -> None:
        def mutate(sources):
            sources["scripts/map/customize.lua"] = sources["scripts/map/customize.lua"].replace('image="trees.tex"', 'image="trees.tex",hideinfrontend=true')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_package(root, mutate=mutate)
            with self.assertRaisesRegex(ValueError, "Unsupported customization option properties: trees"):
                self.module.extract_inventory(root)

    def test_conditional_task_registration_is_not_treated_as_unconditional(self) -> None:
        def mutate(sources):
            sources["scripts/map/tasksets/caves.lua"] += '\nif feature_enabled then AddTaskSet("optional", {location="cave"}) end'
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_package(root, mutate=mutate)
            with self.assertRaisesRegex(ValueError, "Unsupported.*registration"):
                self.module.extract_inventory(root)

    def test_conditional_start_registration_is_rejected(self) -> None:
        def mutate(sources):
            sources["scripts/map/startlocations.lua"] = sources["scripts/map/startlocations.lua"].replace('AddStartLocation("default", {location="forest"})', 'if feature_enabled then AddStartLocation("default", {location="forest"}) end')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_package(root, mutate=mutate)
            with self.assertRaisesRegex(ValueError, "Unsupported.*registration"):
                self.module.extract_inventory(root)

    def test_dynamic_description_mutation_is_rejected(self) -> None:
        def mutate(sources):
            sources["scripts/map/customize.lua"] += '\ntable.insert(frequency_descriptions, {text=STRINGS.D,data="new"})'
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_package(root, mutate=mutate)
            with self.assertRaisesRegex(ValueError, "Unsupported.*description"):
                self.module.extract_inventory(root)

    def test_cli_write_refreshes_only_inventory_and_still_reports_schema_drift(self) -> None:
        cli = importlib.import_module("verify_dst_world_options")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            install = root / "package"
            install.mkdir()
            write_package(install)
            module = root / "modules/dontstarve"
            module.mkdir(parents=True)
            schema = module / "schema.json"
            schema.write_text('{"properties":{}}\n', encoding="utf-8")
            before = schema.read_bytes()
            output = io.StringIO()
            with contextlib.redirect_stderr(output):
                code = cli.main(["--install-root", str(install), "--write"], root=root)
            self.assertEqual(code, 1)
            self.assertIn("mastergen: missing trees", output.getvalue())
            self.assertEqual(schema.read_bytes(), before)
            self.assertEqual(json.loads((module / "world-options.json").read_text())["gameVersion"], "123456")

    def test_repository_verifier_reports_invalid_json(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            module = root / "modules/dontstarve"
            module.mkdir(parents=True)
            (module / "world-options.json").write_text("{bad", encoding="utf-8")
            self.assertIn("DST world options:", "\n".join(self.module.verify_repository(root)))


if __name__ == "__main__":
    unittest.main()
