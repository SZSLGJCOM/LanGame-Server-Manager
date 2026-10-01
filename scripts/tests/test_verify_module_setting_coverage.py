from __future__ import annotations

from collections.abc import Callable
import re
import tempfile
import unittest
from pathlib import Path
import sys
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from scripts import verify_module_setting_coverage as verifier


class UnsignedAccountHashMetadataTests(unittest.TestCase):
    def property(self) -> dict:
        return {
            "type": "string", "format": "textarea",
            "description": "Written to the native bans array in enshrouded_server.json on the next server start.",
            "x-lsgm-player-access-kind": "block",
            "x-lsgm-player-access-codec": "uint64",
            "x-lsgm-player-access-entry-separators": ["newline", "comma"],
            "x-lsgm-player-access-sync": {"mode": "restart"},
        }

    def test_uint64_roster_declares_its_supported_separators(self) -> None:
        self.assertEqual(verifier.validate_player_access_metadata(
            "enshrouded", {"banned_player_ids": self.property()}
        ), [])

    def test_unsigned_codec_does_not_allow_unknown_codecs_or_separators(self) -> None:
        for key, value in (
            ("x-lsgm-player-access-codec", "uint32"),
            ("x-lsgm-player-access-entry-separators", ["newline", "semicolon"]),
        ):
            with self.subTest(key=key):
                property_def = {**self.property(), key: value}
                self.assertTrue(verifier.validate_player_access_metadata(
                    "enshrouded", {"banned_player_ids": property_def}
                ))

    def test_native_hash_renderer_retains_exact_numbers_and_error_propagation(self) -> None:
        self.assertEqual(verifier.validate_json_account_roster_rendering(), [])

    def test_native_hash_guard_detects_rounding_and_removed_validation(self) -> None:
        source = verifier.read_app_storage_templates_rs()
        for marker in (
            "Value::Number(serde_json::Number::from(account_id))",
            "value.parse::<u64>().ok()?",
            "render_enshrouded_banned_accounts_json(context.settings)?",
        ):
            with self.subTest(marker=marker):
                self.assertIn(marker, source)
                with patch.object(verifier, "_APP_STORAGE_TEMPLATES_TEXT", source.replace(marker, "", 1)):
                    self.assertTrue(verifier.validate_json_account_roster_rendering())


class DesktopCommandSourceReaderTests(unittest.TestCase):
    def test_reads_included_command_implementations_without_unreferenced_siblings(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source_root = Path(directory)
            sources = {
                "commands.rs": 'include!("commands_assistant_ops/runtime_dispatch.rs");\n',
                "commands_assistant_ops.rs": 'include!("commands_assistant_ops/runtime_dispatch.rs");\n',
                "commands_assistant_ops/runtime_dispatch.rs": (
                    'fn dispatch_command() {}\ninclude!("nested/transport.rs");\n'
                ),
                "commands_assistant_ops/nested/transport.rs": "fn command_transport() {}\n",
                "commands_assistant_ops/unreferenced.rs": "fn unused_command() {}\n",
            }
            for relative_path, source in sources.items():
                path = source_root / relative_path
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")

            with patch.object(verifier, "DESKTOP_TAURI_COMMANDS_RS", source_root / "commands.rs"), \
                    patch.object(verifier, "_DESKTOP_TAURI_COMMANDS_TEXT", None):
                combined = verifier.read_desktop_tauri_commands_rs()

        self.assertEqual(combined.count("fn dispatch_command()"), 1)
        self.assertIn("fn command_transport()", combined)
        self.assertNotIn("fn unused_command()", combined)

    def test_missing_included_command_implementation_fails_source_read(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            command_path = Path(directory) / "commands.rs"
            command_path.write_text('include!("missing.rs");\n', encoding="utf-8")
            with patch.object(verifier, "DESKTOP_TAURI_COMMANDS_RS", command_path), \
                    patch.object(verifier, "_DESKTOP_TAURI_COMMANDS_TEXT", None):
                with self.assertRaises(FileNotFoundError):
                    verifier.read_desktop_tauri_commands_rs()


class PlayerCenterCoverageVerifierTests(unittest.TestCase):
    def _write_current_player_center_sources(self, root: Path) -> None:
        contracts = {
            "views/servers/PlayerCenterWorkbench.tsx": "export function PlayerCenterWorkbench() {}\n",
            "views/servers/player-center/use-player-access.tsx": "export function usePlayerAccess() {}\n",
            "views/servers/player-center/PlayerAccessRosterEditor.tsx": "export function PlayerAccessRosterEditor() {}\n",
            "views/servers/player-center/PlayerAccessRosterList.tsx": "export function PlayerAccessRosterList() {}\n",
            "views/servers/player-center/SelectedPlayerRosterActions.tsx": "export function SelectedPlayerRosterActions() {}\n",
            "views/servers/player-center/player-access-roster-model.ts": "export function readPlayerAccessRosterCapabilities() {}\nexport function buildRosterFields() {}\n",
            "views/servers/player-center/player-access-roster-validation.ts": "export function validateRosterEntryInput() {}\n",
            "views/servers/player-center/manual-player-action-model.ts": "export function readManualPlayerActions() {}\n",
            "views/servers/player-center/ManualPlayerActions.tsx": "export function ManualPlayerActions() {}\n",
            "views/servers/player-center/OnlinePlayersView.tsx": "export function OnlinePlayersView() {}\n",
            "views/servers/player-center/LivePlayerActionPanel.tsx": "export function LivePlayerActionPanel() {}\n",
            "views/servers/player-center/LivePlayerTable.tsx": "export function LivePlayerTable() {}\n",
            "views/servers/player-center/LivePlayerState.tsx": "export function LivePlayerState() {}\n",
            "views/servers/player-center/use-live-players.ts": "export function useLivePlayers() {}\n",
        }
        for relative_path, source in contracts.items():
            path = root / relative_path
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(source, encoding="utf-8", newline="\n")

    def test_reads_current_player_center_sources_without_the_deleted_monolith(self) -> None:
        self.assertTrue(
            hasattr(verifier, "PLAYER_CENTER_FRONTEND_FILES"),
            "the coverage verifier must name the current Player Center source set",
        )
        with tempfile.TemporaryDirectory() as directory:
            source_root = Path(directory)
            self._write_current_player_center_sources(source_root)

            sources = verifier.read_player_center_frontend_sources(source_root)

        self.assertEqual(set(sources), set(verifier.PLAYER_CENTER_FRONTEND_FILES))
        self.assertIn("views/servers/PlayerCenterWorkbench.tsx", sources)
        self.assertNotIn("views/servers/PlayerAccessWorkbench.tsx", sources)

    def test_reports_a_missing_current_player_center_file_as_validation_error(self) -> None:
        self.assertTrue(
            hasattr(verifier, "validate_player_center_frontend_contracts"),
            "the coverage verifier must validate the Player Center boundary separately",
        )
        with tempfile.TemporaryDirectory() as directory:
            source_root = Path(directory)
            self._write_current_player_center_sources(source_root)
            (source_root / "views/servers/player-center/LivePlayerState.tsx").unlink()

            failures = verifier.validate_player_center_frontend_contracts(source_root)

        self.assertEqual(
            failures,
            ["frontend: missing Player Center source views/servers/player-center/LivePlayerState.tsx"],
        )

    def test_accepts_the_public_player_center_contracts_from_focused_files(self) -> None:
        self.assertTrue(
            hasattr(verifier, "validate_player_center_frontend_contracts"),
            "the coverage verifier must validate the Player Center boundary separately",
        )
        with tempfile.TemporaryDirectory() as directory:
            source_root = Path(directory)
            self._write_current_player_center_sources(source_root)

            failures = verifier.validate_player_center_frontend_contracts(source_root)

        self.assertEqual(failures, [])

    def test_accepts_the_documented_no_whitespace_raw_target_marker(self) -> None:
        self.assertIsNotNone(
            verifier.PLAYER_ACTION_SINGLE_TOKEN_TARGET_RE.search(
                "Player name (no-whitespace)"
            )
        )


class NativeTemplateTokenCoverageVerifierTests(unittest.TestCase):
    def test_counts_xml_escaped_setting_tokens_as_native_writer_references(self) -> None:
        referenced, unknown = verifier.referenced_settings(
            "barotrauma",
            '{{xml.settings.server_name}} {{json.settings.metadata}}',
            {"server_name", "metadata"},
        )

        self.assertEqual(referenced, {"server_name", "metadata"})
        self.assertEqual(unknown, set())

    def test_shared_extra_args_token_maps_to_the_module_schema_field(self) -> None:
        referenced, unknown = verifier.referenced_settings(
            "theforest",
            "{{launch.extra_args}}",
            {"extra_launch_args"},
        )

        self.assertEqual(referenced, {"extra_launch_args"})
        self.assertEqual(unknown, set())

    def test_shared_extra_args_token_rejects_a_missing_runtime_resolver(self) -> None:
        source = verifier.read_app_runtime_launch_templates_rs()
        marker = 'token.strip_prefix("launch.")'
        self.assertIn(marker, source)
        original_cache = verifier._APP_RUNTIME_LAUNCH_TEMPLATES_TEXT
        verifier._APP_RUNTIME_LAUNCH_TEMPLATES_TEXT = source.replace(marker, "removed", 1)
        try:
            referenced, unknown = verifier.referenced_settings(
                "theforest",
                "{{launch.extra_args}}",
                {"extra_launch_args"},
            )
        finally:
            verifier._APP_RUNTIME_LAUNCH_TEMPLATES_TEXT = original_cache

        self.assertEqual(referenced, set())
        self.assertEqual(unknown, {"launch.extra_args"})


class IndependentPlayerCapabilitiesTests(unittest.TestCase):
    def test_romestead_read_only_player_list_does_not_require_moderation(self) -> None:
        module = verifier.read_module_toml("romestead")
        self.assertTrue(verifier.module_has_online_player_list(module))
        self.assertFalse(verifier.module_has_runtime_player_mutating_action(module))
        self.assertEqual(verifier.validate_player_management_contract(
            "romestead", module, verifier.read_schema_property_defs("romestead")
        ), [])

    def test_read_only_runtime_actions_require_a_real_list_declaration(self) -> None:
        module = verifier.read_module_toml("romestead")
        del module["runtime"]["player_list"]
        failures = verifier.validate_player_management_contract("romestead", module, {})
        self.assertTrue(any("declared online player-list adapter" in failure for failure in failures))

    def test_pending_console_adapter_keeps_independent_persistent_roster(self) -> None:
        module = verifier.read_module_toml("barotrauma")
        schema = verifier.read_schema_property_defs("barotrauma")
        # Model an unavailable transport independently of the deployed ConPTY collector.
        module["runtime"].pop("player_list", None)
        module["runtime"].pop("player_actions", None)
        module["player_management"].update({
            "status": "pending_adapter",
            "reason": "A reliable console transport is not yet verified. Persistent admins are rendered into Data/clientpermissions.xml.",
            "verification": "Runtime commands stay hidden until their transport is verified.",
        })
        self.assertTrue(verifier.module_has_player_roster_management_fields(schema))
        self.assertEqual(verifier.validate_player_management_contract("barotrauma", module, schema), [])

    def test_barotrauma_console_list_and_persistent_roster_coexist(self) -> None:
        module = verifier.read_module_toml("barotrauma")
        schema = verifier.read_schema_property_defs("barotrauma")
        self.assertTrue(verifier.module_has_online_player_list(module))
        self.assertTrue(verifier.module_has_player_roster_management_fields(schema))
        self.assertFalse(verifier.module_has_runtime_player_mutating_action(module))
        self.assertEqual(verifier.validate_player_management_contract("barotrauma", module, schema), [])


class RosterSettingsBackendSaveChainVerifierTests(unittest.TestCase):
    representative_source = """
pub async fn update_instance_record_if_current(
    state: State,
    input: UpdateInput,
    expected_settings_json: String,
) -> Result {
    update_instance_record_with_precondition(
        state,
        input,
        Some(expected_settings_json.as_str()),
        None,
    )
    .await
}

async fn update_instance_record_with_precondition(
    state: State,
    input: UpdateInput,
    expected_settings_json: Option<&str>,
    expected_instance: Option<InstanceDetails>,
) -> Result {
    match expected_settings_json {
        Some(baseline) => update_instance_if_current(&renamed_paths, input, baseline).await,
        None => update_instance(&renamed_paths, input).await,
    }
}
"""

    @classmethod
    def setUpClass(cls) -> None:
        cls.commands_source = verifier.read_desktop_tauri_commands_rs()

    def test_accepts_the_current_conditional_update_dispatch(self) -> None:
        self.assertEqual(
            verifier.validate_roster_settings_backend_save_chain(self.commands_source),
            [],
        )

    def test_rejects_dropping_the_expected_settings_baseline(self) -> None:
        mutated = self.representative_source.replace(
            "Some(expected_settings_json.as_str())", "None", 1
        )

        failures = verifier.validate_roster_settings_backend_save_chain(mutated)

        self.assertIn(
            "backend: roster/settings conditional update must forward the expected settings baseline",
            failures,
        )

    def test_rejects_routing_the_expected_branch_to_an_unconditional_update(self) -> None:
        mutated = self.representative_source.replace(
            "Some(baseline) => update_instance_if_current(&renamed_paths, input, baseline).await",
            "Some(_baseline) => update_instance(&renamed_paths, input).await",
            1,
        )

        failures = verifier.validate_roster_settings_backend_save_chain(mutated)

        self.assertIn(
            "backend: roster/settings expected-baseline branch must use conditional instance update",
            failures,
        )


class RosterSettingsFrontendSaveChainVerifierTests(unittest.TestCase):
    call = "await updateInstance(input, expectedSettingsJson, saveOptions.collectionRemoval)"

    def source(self, call: str) -> str:
        return (
            "async function handleSaveSettings(input: UpdateInstanceInput, "
            "saveOptions: SaveInstanceSettingsOptions = {}) {\n"
            f"  const savedDetails = {call};\n"
            "}"
        )

    def test_accepts_current_save_chain_and_multiline_calls(self) -> None:
        for source in (
            verifier.read_frontend_file("hooks/useDesktopActions.ts"),
            self.source(self.call),
            self.source("await updateInstance(\n input,\n expectedSettingsJson,\n saveOptions.collectionRemoval,\n)"),
        ):
            with self.subTest(source=source[:80]):
                self.assertEqual(verifier.validate_roster_settings_frontend_save_chain(source), [])

    def test_rejects_missing_unawaited_commented_or_quoted_save(self) -> None:
        for call in (
            "removed_update()",
            self.call.removeprefix("await "),
            f"/* {self.call} */ removed_update()",
            repr(self.call),
        ):
            with self.subTest(call=call):
                self.assertTrue(verifier.validate_roster_settings_frontend_save_chain(self.source(call)))

    def test_rejects_disconnected_or_incorrect_save_arguments(self) -> None:
        for source in (
            self.source("removed_update()") + f"\nasync function unrelated() {{ {self.call}; }}",
            self.source(self.call.replace("input,", "otherInput,")),
            self.source(self.call.replace("expectedSettingsJson", "undefined")),
            self.source(self.call.replace("saveOptions.collectionRemoval", "undefined")),
        ):
            with self.subTest(source=source):
                self.assertTrue(verifier.validate_roster_settings_frontend_save_chain(source))


class WorkshopMaterializationCoverageVerifierTests(unittest.TestCase):
    entrypoints = {
        "barotrauma": ("materialize_barotrauma_workshop_mods", "prepare_barotrauma_workshop_mods"),
        "conanexiles": ("materialize_conan_modlist", "prepare_conan_modlist"),
    }

    @classmethod
    def setUpClass(cls) -> None:
        cls.source = verifier.read_app_storage_templates_rs()

    def assert_workshop_coverage(self, source: str, expected: bool, module_id: str) -> None:
        with patch.object(verifier, "_APP_STORAGE_TEMPLATES_TEXT", source):
            referenced = verifier.materialized_settings(module_id, {"mod_workshop_ids"})
        self.assertEqual("mod_workshop_ids" in referenced, expected)

    def test_tracks_current_instance_settings_through_the_package_resolver(self) -> None:
        for module_id in self.entrypoints:
            with self.subTest(module_id=module_id):
                self.assert_workshop_coverage(self.source, True, module_id)

    def test_rejects_missing_commented_or_other_instance_resolver_calls(self) -> None:
        for module_id, (_, prepare) in self.entrypoints.items():
            body = verifier.rust_function_body_from_source(self.source, prepare)
            match = re.search(r"\bresolve_packages\s*\(", body)
            self.assertIsNotNone(match)
            arguments = verifier.extract_ts_balanced_block(body, match.end() - 1, "(", ")")
            self.assertTrue(arguments)
            call = body[match.start():match.end() - 1] + arguments
            for replacement in (
                "removed_resolver()",
                f"/* {call} */ removed_resolver()",
                call.replace("context,", "other_context,", 1),
            ):
                with self.subTest(module_id=module_id, replacement=replacement):
                    changed = body.replace(call, replacement, 1)
                    self.assertNotEqual(changed, body)
                    self.assert_workshop_coverage(self.source.replace(body, changed, 1), False, module_id)

    def test_rejects_disconnected_preparation_or_file_application(self) -> None:
        for module_id, (entrypoint, prepare) in self.entrypoints.items():
            body = verifier.rust_function_body_from_source(self.source, entrypoint)
            call = f"{prepare}(context)"
            for changed in (
                body.replace(call, "removed_preparation()", 1),
                body.replace(call, f"/* {call} */ removed_preparation()", 1),
                body.replace(call, f"{prepare}(other_context)", 1),
                body.replace("files.apply(", "removed_application(", 1),
                body.replace("files.apply(", "/* files.apply( */ removed_application(", 1),
            ):
                with self.subTest(module_id=module_id, body=changed):
                    self.assertNotEqual(changed, body)
                    self.assert_workshop_coverage(self.source.replace(body, changed, 1), False, module_id)

    def test_rejects_removed_commented_or_other_instance_setting_reads(self) -> None:
        body = verifier.rust_function_body_from_source(self.source, "resolve_packages")
        match = re.search(
            r'\bparse_workshop_id_list\s*\(\s*context\.settings\s*,\s*"mod_workshop_ids"\s*,?\s*\)',
            body,
        )
        self.assertIsNotNone(match)
        call = match.group(0)
        for replacement in (
            "Vec::new()",
            f"/* {call} */ Vec::new()",
            call.replace("context.settings", "other_context.settings"),
            call.replace("mod_workshop_ids", "unused_workshop_ids"),
        ):
            changed = body.replace(call, replacement, 1)
            self.assertNotEqual(changed, body)
            for module_id in self.entrypoints:
                with self.subTest(module_id=module_id, replacement=replacement):
                    self.assert_workshop_coverage(self.source.replace(body, changed, 1), False, module_id)


class ServerDetailToolTabCoverageTests(unittest.TestCase):
    view = '''
import { buildServerDetailTabSpecs } from "./servers/server-detail-tab-specs";
import { ServerDetailTabs } from "./servers/ServerDetailTabs";
import { PlayerCenterWorkbench } from "./servers/PlayerCenterWorkbench";
import { GMToolsWorkbench } from "./servers/GMToolsWorkbench";
type ViewProps = {
  selectedDetails: { summary: { id: string; module_id: string } } | null;
  activeTab: string;
  retainedInstanceId: string | null;
};
export function ServersView(props: ViewProps) {
  const detailTabs = buildServerDetailTabSpecs({ moduleId: props.selectedDetails?.summary.module_id ?? "" });
  const activeDetailTab = props.activeTab;
  const retainedGmInstanceId = props.retainedInstanceId;
  return <>
    <ServerDetailTabs tabs={detailTabs} activeTab={activeDetailTab} />
    {activeDetailTab === "players" && props.selectedDetails ? (
      <PlayerCenterWorkbench details={props.selectedDetails} />
    ) : null}
    {activeDetailTab === "gm" && props.selectedDetails ? (
      <GMToolsWorkbench details={props.selectedDetails} />
    ) : null}
  </>;
}
'''
    specs = '''
type TabSpec = { id: string; disabled?: boolean };
export function buildServerDetailTabSpecs(input: { moduleId: string }): TabSpec[] {
  const hasGmTools = input.moduleId !== "unsupported";
  return [
    { id: "runtime" },
    { id: "settings" },
    { id: "mods" },
    { id: "players" },
    { id: "maintenance" },
    { id: "gm", disabled: !hasGmTools }
  ];
}
'''
    direct_gm_branch = '''{activeDetailTab === "gm" && props.selectedDetails ? (
      <GMToolsWorkbench details={props.selectedDetails} />
    ) : null}'''
    retained_gm_branch = '''{props.selectedDetails && (activeDetailTab === "gm" || retainedGmInstanceId === props.selectedDetails.summary.id) ? (
      <div hidden={activeDetailTab !== "gm"}>
        <GMToolsWorkbench details={props.selectedDetails} />
      </div>
    ) : null}'''

    def changed(self, source: str, before: str, after: str) -> str:
        self.assertEqual(source.count(before), 1)
        return source.replace(before, after, 1)

    def check(self, view: str | None = None, specs: str | None = None) -> list[str]:
        return verifier.validate_server_detail_tool_tabs(
            self.view if view is None else view,
            self.specs if specs is None else specs,
        )

    def test_current_split_builder_is_connected_to_the_visible_workbenches(self) -> None:
        self.assertEqual(verifier.validate_server_detail_tool_tabs(
            verifier.read_frontend_file("views/ServersView.tsx"),
            verifier.read_frontend_file("views/servers/server-detail-tab-specs.ts"),
        ), [])

    def test_fixed_direct_tab_fixture_is_accepted(self) -> None:
        self.assertEqual(self.check(), [])

    def test_retained_instance_tools_are_visible_only_on_the_gm_tab(self) -> None:
        view = self.changed(self.view, self.direct_gm_branch, self.retained_gm_branch)
        self.assertEqual(self.check(view=view), [])
        split_tools = self.changed(view, '<GMToolsWorkbench details={props.selectedDetails} />', '''
          {isArkModule(props.selectedDetails.summary.module_id)
            ? <ArkCreatureSpawner details={props.selectedDetails} />
            : <GMToolsWorkbench details={props.selectedDetails} />}
        ''')
        self.assertEqual(self.check(view=split_tools), [])

    def test_retained_tools_still_require_the_selected_tab_instance_and_visibility(self) -> None:
        view = self.changed(self.view, self.direct_gm_branch, self.retained_gm_branch)
        for before, after in (
            ('activeDetailTab === "gm"', 'activeDetailTab === "unused"'),
            ('retainedGmInstanceId === props.selectedDetails.summary.id', 'retainedGmInstanceId === otherInstanceId'),
            ('hidden={activeDetailTab !== "gm"}', 'hidden={activeDetailTab === "gm"}'),
            ('hidden={activeDetailTab !== "gm"}', 'hidden={true}'),
            ('<GMToolsWorkbench', '<UnusedGMToolsWorkbench'),
        ):
            with self.subTest(after=after):
                self.assertEqual(self.check(view=self.changed(view, before, after)), [
                    "frontend: ServersView must render GMToolsWorkbench in the gm tab"
                ])

    def test_missing_builder_import_or_call_cannot_be_replaced_by_a_comment(self) -> None:
        imported = 'import { buildServerDetailTabSpecs } from "./servers/server-detail-tab-specs";'
        for before, after in (
            (imported, f"/* {imported} */"),
            (imported, imported.replace("server-detail-tab-specs", "unused-tab-specs")),
            ("= buildServerDetailTabSpecs(", "= unusedTabSpecs("),
            ("= buildServerDetailTabSpecs(", "= /* buildServerDetailTabSpecs( */ unusedTabSpecs("),
        ):
            with self.subTest(after=after):
                self.assertIn(
                    "frontend: ServersView must import and render the result of buildServerDetailTabSpecs",
                    self.check(view=self.changed(self.view, before, after)),
                )

    def test_unused_builder_result_does_not_prove_visible_tabs(self) -> None:
        view = self.changed(self.view, "tabs={detailTabs}", "tabs={otherTabs}")
        self.assertIn(
            "frontend: ServersView must import and render the result of buildServerDetailTabSpecs",
            self.check(view=view),
        )

    def test_builder_call_in_an_unrelated_component_does_not_count(self) -> None:
        view = self.changed(self.view, "export function ServersView(", "export function UnusedServersView(")
        view += "\nexport function ServersView() { return null; }\n"
        self.assertIn(
            "frontend: ServersView must import and render the result of buildServerDetailTabSpecs",
            self.check(view=view),
        )

    def test_each_permanent_tab_is_required_in_the_actual_builder(self) -> None:
        for tab_id in ("gm", "players"):
            with self.subTest(tab_id=tab_id):
                specs = self.changed(self.specs, f'id: "{tab_id}"', 'id: "unused"')
                specs += f'\nconst unrelated = [{{ id: "{tab_id}" }}];\n'
                self.assertIn(
                    f"frontend: server detail tab builder must permanently return the {tab_id} tab",
                    self.check(specs=specs),
                )

    def test_conditional_or_filtered_tab_arrays_do_not_count_as_permanent(self) -> None:
        for before, after in (
            ("return [", "if (hasGmTools) return ["),
            ("return [", "return [...(hasGmTools ? ["),
            ("\n  ];", "\n  ].filter((tab) => hasGmTools);"),
        ):
            with self.subTest(after=after):
                specs = self.changed(self.specs, before, after)
                if after.endswith("? ["):
                    specs = self.changed(specs, "\n  ];", "\n  ] : [])];")
                for tab_id in ("gm", "players"):
                    self.assertIn(
                        f"frontend: server detail tab builder must permanently return the {tab_id} tab",
                        self.check(specs=specs),
                    )

    def test_nested_metadata_is_not_a_top_level_tab_id(self) -> None:
        specs = self.changed(self.specs, 'id: "players"', 'metadata: { id: "players" }, id: "unused"')
        self.assertIn(
            "frontend: server detail tab builder must permanently return the players tab",
            self.check(specs=specs),
        )

    def test_unused_or_commented_builder_cannot_supply_tab_ids(self) -> None:
        for specs in (
            f"/* {self.specs} */",
            self.changed(self.specs, "function buildServerDetailTabSpecs(", "function unusedTabSpecs("),
        ):
            with self.subTest(specs=specs[:80]):
                self.assertEqual(self.check(specs=specs), [
                    "frontend: server detail tab builder must permanently return the gm tab",
                    "frontend: server detail tab builder must permanently return the players tab",
                ])

    def test_each_workbench_must_render_in_its_matching_tab_branch(self) -> None:
        for tab_id, component in (("gm", "GMToolsWorkbench"), ("players", "PlayerCenterWorkbench")):
            for before, after in (
                (f"<{component}", f"<Unused{component}"),
                (f"<{component}", f"/* <{component} */ <Unused{component}"),
                (f'activeDetailTab === "{tab_id}" &&', f'activeDetailTab === "unused" &&'),
            ):
                with self.subTest(tab_id=tab_id, after=after):
                    self.assertIn(
                        f"frontend: ServersView must render {component} in the {tab_id} tab",
                        self.check(view=self.changed(self.view, before, after)),
                    )


class InstanceSettingsLockCoverageVerifierTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.path = verifier.APP_STORAGE_INSTANCES_RS.parent / "instance_settings_lock.rs"
        cls.source = cls.path.read_text(encoding="utf-8-sig")

    def check_source(self, source: str) -> list[str]:
        read_text = Path.read_text

        def read(path: Path, *args, **kwargs):
            return source if path == self.path else read_text(path, *args, **kwargs)

        with patch.object(Path, "read_text", read):
            return verifier.validate_frontend_coverage()

    def test_accepts_exclusive_mutation_dispatch_with_shared_read_support(self) -> None:
        self.assertEqual(self.check_source(self.source), [])

    def test_rejects_dispatching_mutations_to_a_shared_lock(self) -> None:
        body = verifier.rust_function_body_from_source(self.source, "acquire_lock_at_path")
        self.assertIn("LockMode::Exclusive", body)
        changed = body.replace("LockMode::Exclusive", "LockMode::Shared", 1)
        self.assertEqual(self.check_source(self.source.replace(body, changed, 1)), [
            "backend: instance settings lock must acquire an exclusive nonblocking lease"
        ])

    def test_rejects_locking_a_different_instance_path(self) -> None:
        mutated = self.source.replace(
            "acquire_lock_at_path(instance_settings_mutation_lock_path(paths, instance_id))",
            "acquire_lock_at_path(module_instance_creation_lock_path(paths, instance_id))",
            1,
        )
        self.assertNotEqual(mutated, self.source)
        self.assertEqual(self.check_source(mutated), [
            "backend: settings mutations must acquire the per-instance lock path"
        ])

    def test_rejects_shared_or_commented_exclusive_acquisition(self) -> None:
        for replacement in (
            "file.try_lock_shared()",
            "/* file.try_lock() */ Ok(())",
        ):
            with self.subTest(replacement=replacement):
                body = verifier.rust_function_body_from_source(self.source, "acquire_lock_at_path_with_mode")
                self.assertIn("file.try_lock()", body)
                changed = body.replace("file.try_lock()", replacement, 1)
                mutated = self.source.replace(body, changed, 1)
                self.assertNotEqual(mutated, self.source)
                self.assertEqual(self.check_source(mutated), [
                    "backend: instance settings lock must acquire an exclusive nonblocking lease"
                ])

    def test_rejects_granting_a_lease_when_the_lock_is_contended(self) -> None:
        for old, new in (
            ("TryLockError::WouldBlock => LockAcquireError::Contended", "TryLockError::WouldBlock => LockAcquireError::Io(error.into())"),
            ("Err(StorageError::InstanceSettingsLocked { path: lock_path })", "Ok(InstanceSettingsLock::new(file))"),
        ):
            with self.subTest(marker=old):
                mutated = self.source.replace(old, "/* " + old + " */ " + new, 1)
                self.assertNotEqual(mutated, self.source)
                self.assertEqual(self.check_source(mutated), [
                    "backend: instance settings lock must report contention without granting a lease"
                ])

    def test_rejects_discarding_the_lock_error_mapping_result(self) -> None:
        body = verifier.rust_function_body_from_source(self.source, "map_try_lock_result")
        self.assertTrue(body)
        changed = body.replace("result.map_err(", "let _ignored = result.map_err(", 1)
        changed = changed.replace("    })", "    });\n    Ok(())", 1)
        self.assertNotEqual(changed, body)
        self.assertEqual(self.check_source(self.source.replace(body, changed, 1)), [
            "backend: instance settings lock must report contention without granting a lease"
        ])


class DstNativeSettingsCoverageVerifierTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.storage_instances_source = verifier.APP_STORAGE_INSTANCES_RS.read_text(
            encoding="utf-8-sig"
        )
        cls.storage_templates_source = verifier.read_app_storage_templates_rs()
        cls.runtime_launch_source = verifier.APP_RUNTIME_LAUNCH_TEMPLATES_RS.read_text(
            encoding="utf-8-sig"
        )

    def test_accepts_current_dst_native_inventory_commit_and_token_contracts(self) -> None:
        self.assertEqual(verifier.validate_dst_native_settings_contract(verifier.ROOT), [])

    def test_function_extraction_does_not_match_a_longer_name(self) -> None:
        source = "fn write_config_in_worker() { wrong(); } fn write_config() { correct(); }"
        self.assertEqual(
            verifier.rust_function_body_from_source(source, "write_config").strip(),
            "correct();",
        )

    def test_function_extraction_preserves_generic_and_lifetime_declarations(self) -> None:
        source = "fn render_config<'a, T>(input: &'a T) { render(input); }"
        self.assertEqual(
            verifier.rust_function_body_from_source(source, "render_config").strip(),
            "render(input);",
        )

    def test_rejects_cancellable_transaction_ownership(self) -> None:
        mutated = self.storage_instances_source.replace(".complete_mutation(", ".run_directly(", 1)
        failures = verifier.validate_dst_native_settings_contract(
            verifier.ROOT, storage_instances_source=mutated
        )
        self.assertTrue(any("retain mutation ownership" in failure for failure in failures), failures)

    def test_rejects_a_worker_that_does_not_retain_the_mutation_lease(self) -> None:
        marker = "settings_lock\n        .spawn_blocking(move || {"
        self.assertIn(marker, self.storage_templates_source)
        mutated = self.storage_templates_source.replace(marker, "tokio::task::spawn_blocking(move || {", 1)
        failures = verifier.validate_dst_native_settings_contract(
            verifier.ROOT, storage_templates_source=mutated
        )
        self.assertTrue(any("retain the instance mutation lease" in failure for failure in failures), failures)

    def test_checks_deletion_ownership_in_its_own_module(self) -> None:
        source = (verifier.APP_STORAGE_INSTANCES_RS.parent / "instance_deletion.rs").read_text(
            encoding="utf-8-sig"
        )
        self.assertIn(".complete_mutation(", source)
        failures = verifier.validate_dst_native_settings_contract(
            verifier.ROOT,
            storage_deletion_source=source.replace(".complete_mutation(", ".run_directly(", 1),
        )
        self.assertEqual(failures, [
            "dontstarve: delete_instance must retain mutation ownership until its transaction finishes"
        ])

    def mutate_template_function(self, function: str, mutation: Callable[[str], str]) -> str:
        body = verifier.rust_function_body_from_source(
            self.storage_templates_source, function
        )
        self.assertTrue(body, f"missing function {function}")
        mutated = mutation(body)
        self.assertNotEqual(mutated, body)
        return self.storage_templates_source.replace(body, mutated, 1)

    def mutate_instance_function(self, function: str, mutation: Callable[[str], str]) -> str:
        body = verifier.rust_function_body_from_source(self.storage_instances_source, function)
        self.assertTrue(body, f"missing function {function}")
        changed = mutation(body)
        self.assertNotEqual(changed, body)
        return self.storage_instances_source.replace(body, changed, 1)

    def test_rejects_missing_commented_or_unawaited_creation_delegate(self) -> None:
        body = verifier.rust_function_body_from_source(
            self.storage_instances_source, "create_instance_transaction"
        )
        match = re.search(
            r"\bcreate_instance_in_pool\s*\(\s*paths\s*,\s*descriptor\s*,\s*input\s*,\s*"
            r"options\s*,\s*module_creation_lock\s*,\s*&pool\s*,?\s*\)\s*\.await\b",
            body,
        )
        self.assertIsNotNone(match, "creation must delegate its transaction inputs and await completion")
        call = match.group(0)
        for replacement in ("removed_creation_delegate()", "/* " + call + " */ removed_creation_delegate()", call.replace(".await", "")):
            with self.subTest(replacement=replacement):
                mutated = self.mutate_instance_function(
                    "create_instance_transaction", lambda source: source.replace(call, replacement, 1)
                )
                self.assertIn(
                    "dontstarve: create_instance_transaction must await create_instance_in_pool with its transaction inputs",
                    verifier.validate_dst_native_settings_contract(
                        verifier.ROOT, storage_instances_source=mutated
                    ),
                )

    def test_accepts_private_setup_formatting_and_local_variable_names(self) -> None:
        body = verifier.rust_function_body_from_source(
            self.storage_templates_source, "sync_dst_mod_setup"
        )
        variants = (
            body.replace("(instance_root)?", "(\n instance_root,\n)?", 1),
            body.replace("instance_root", "owned_root").replace("let runtime =", "let resolved =").replace("runtime !=", "resolved !=").replace("root: runtime", "root: resolved"),
        )
        for mutated in variants:
            with self.subTest(body=mutated):
                self.assertNotEqual(mutated, body)
                self.assertEqual(verifier.validate_dst_native_settings_contract(
                    verifier.ROOT,
                    storage_templates_source=self.storage_templates_source.replace(body, mutated, 1),
                ), [])

    def test_rejects_private_root_bypass_or_mismatch_guard_bypass(self) -> None:
        resolver = "crate::program_runtime::resolve_instance_runtime_root(instance_root)?"
        for old, new in (
            (resolver, "/* " + resolver + " */ context.install_root.to_path_buf()"),
            (resolver, resolver[:-1] + ".unwrap_or_else(|_| context.install_root.to_path_buf())"),
            ("if runtime != context.install_root", "if runtime == context.install_root"),
            ("return Err(StorageError::UnsafeManagedPath", "let _ignored = Err(StorageError::UnsafeManagedPath"),
        ):
            with self.subTest(replacement=new):
                mutated = self.mutate_template_function("sync_dst_mod_setup", lambda body: body.replace(old, new, 1))
                self.assertEqual(verifier.validate_dst_native_settings_contract(
                    verifier.ROOT, storage_templates_source=mutated
                ), ["dontstarve: setup must validate the instance private runtime before writing"])

    def test_rejects_setup_without_independent_program_ownership(self) -> None:
        for replacement in ("== crate::InstanceProgramMode::Independent", "!= crate::InstanceProgramMode::Shared"):
            with self.subTest(replacement=replacement):
                mutated = self.mutate_template_function(
                    "sync_dst_mod_setup",
                    lambda body: body.replace("!= crate::InstanceProgramMode::Independent", replacement, 1),
                )
                self.assertEqual(verifier.validate_dst_native_settings_contract(
                    verifier.ROOT, storage_templates_source=mutated,
                ), ["dontstarve: setup must validate the instance private runtime before writing"])

    def test_rejects_setup_write_before_private_runtime_validation(self) -> None:
        call = "write_dst_mod_setup(context.install_root, context.settings, files)"
        mutated = self.mutate_template_function(
            "sync_dst_mod_setup", lambda body: call + "?;\n" + body.replace(call, "Ok(())", 1)
        )
        self.assertEqual(verifier.validate_dst_native_settings_contract(
            verifier.ROOT, storage_templates_source=mutated
        ), ["dontstarve: setup must validate the instance private runtime before writing"])

    def test_rejects_setup_using_shared_root_or_other_instance_settings(self) -> None:
        for old, new in (
            ("context.install_root, context.settings", "context.shared_install_root, context.settings"),
            ("context.settings, files", "other_instance_settings, files"),
            ("context.settings, files", "context.settings, &mut unrelated_files"),
        ):
            with self.subTest(replacement=new):
                mutated = self.mutate_template_function("sync_dst_mod_setup", lambda body: body.replace(old, new, 1))
                self.assertEqual(verifier.validate_dst_native_settings_contract(
                    verifier.ROOT, storage_templates_source=mutated
                ), ["dontstarve: setup must use only this instance's settings and file transaction"])

    def test_rejects_setup_writes_outside_the_file_transaction(self) -> None:
        mutated = self.mutate_template_function(
            "write_dst_mod_setup", lambda body: body.replace("files.write(&setup_path, rendered.as_bytes())", "std::fs::write(&setup_path, rendered.as_bytes())", 1)
        )
        self.assertEqual(verifier.validate_dst_native_settings_contract(
            verifier.ROOT, storage_templates_source=mutated
        ), ["dontstarve: setup renderer output must be written through the supplied file transaction"])

    def test_rejects_disconnected_setup_dispatch_or_discarded_setup_errors(self) -> None:
        marker = '"dontstarve" => sync_dst_mod_setup(context, files)?,'
        for replacement in (
            '"dontstarve" => {},',
            '"dontstarve" => { let _ignored = sync_dst_mod_setup(context, files); },',
        ):
            with self.subTest(replacement=replacement):
                mutated = self.mutate_template_function(
                    "materialize_module_support_files_pending",
                    lambda body: body.replace(marker, replacement, 1),
                )
                self.assertEqual(verifier.validate_dst_native_settings_contract(
                    verifier.ROOT, storage_templates_source=mutated
                ), ["dontstarve: support dispatcher must propagate private setup failures"])

    def test_rejects_running_instance_support_file_rewrites(self) -> None:
        marker = "if !context.instance_running {\n            materialize_module_support_files_pending(context, &mut files, prepared)?;\n        }"
        mutated = self.mutate_template_function(
            "write_pending_configuration_with_native_snapshot", lambda body: body.replace(marker, marker.replace("!context.instance_running", "context.instance_running"), 1)
        )
        self.assertEqual(verifier.validate_dst_native_settings_contract(
            verifier.ROOT, storage_templates_source=mutated
        ), ["dontstarve: support files must stay in the stopped-instance file transaction"])

    def test_rejects_commented_transaction_commit_or_compensation(self) -> None:
        for marker in ("config_mutation.commit();", "config_mutation.rollback_after(original)"):
            with self.subTest(marker=marker):
                mutated = self.storage_instances_source.replace(marker, "/* " + marker + " */", 1)
                self.assertNotEqual(mutated, self.storage_instances_source)
                self.assertEqual(verifier.validate_dst_native_settings_contract(
                    verifier.ROOT, storage_instances_source=mutated
                ), [f"dontstarve: transaction compensation no longer preserves marker {marker!r}"])

    def test_rejects_database_rollback_or_reversed_file_completion_branches(self) -> None:
        body = verifier.rust_function_body_from_source(
            self.storage_instances_source, "commit_instance_transaction"
        )
        variants = (
            body.replace("match tx.commit().await", "match tx.rollback().await", 1),
            body.replace("config_mutation.commit();", "config_mutation.rollback_after(original);", 1)
                .replace(".spawn_blocking(move || config_mutation.rollback_after(original))",
                         ".spawn_blocking(move || { config_mutation.commit(); original })", 1),
        )
        for changed in variants:
            with self.subTest(body=changed):
                self.assertNotEqual(changed, body)
                self.assertEqual(verifier.validate_dst_native_settings_contract(
                    verifier.ROOT,
                    storage_instances_source=self.storage_instances_source.replace(body, changed, 1),
                ), ["dontstarve: database commit must finalize files on success and compensate on failure"])

    def test_tracks_private_setup_workshop_fields_and_detects_removed_consumers(self) -> None:
        fields = {"shared_workshop_mod_ids", "shared_workshop_collection_ids"}
        self.assertEqual(verifier.materialized_settings("dontstarve", fields), fields)
        for field in fields:
            with self.subTest(field=field):
                mutated = self.mutate_template_function("render_dst_mod_setup", lambda body: body.replace(f'"{field}"', '"removed_setting"', 1))
                with patch.object(verifier, "_APP_STORAGE_TEMPLATES_TEXT", mutated):
                    self.assertEqual(verifier.materialized_settings("dontstarve", fields), fields - {field})

    def test_rejects_commit_before_native_configuration_materialization(self) -> None:
        for function, lock in (
            ("create_instance_in_pool", "module_creation_lock"),
            ("update_instance_transaction", "settings_lock"),
            ("materialize_instance_configuration_transaction", "settings_lock"),
        ):
            with self.subTest(function=function):
                marker = f"commit_instance_transaction(tx, config_mutation, {lock}).await?;"
                body = verifier.rust_function_body_from_source(self.storage_instances_source, function)
                self.assertIn(marker, body)
                mutated = self.mutate_instance_function(
                    function, lambda source: marker + source.replace(marker, "", 1)
                )
                self.assertIn(
                    f"dontstarve: {function} must coordinate support files and instance.json before committing its transaction",
                    verifier.validate_dst_native_settings_contract(
                        verifier.ROOT, storage_instances_source=mutated
                    ),
                )

    def test_rejects_deleting_a_unified_instance_config_helper_call(self) -> None:
        marker = "let config_mutation = write_pending_instance_configuration_in_worker("
        self.assertIn(marker, self.storage_instances_source)
        mutated = self.storage_instances_source.replace(
            marker,
            "let config_mutation = removed_unified_instance_config_helper(",
            1,
        )

        failures = verifier.validate_dst_native_settings_contract(
            verifier.ROOT,
            storage_instances_source=mutated,
        )

        self.assertTrue(
            any("must coordinate support files and instance.json" in failure for failure in failures),
            failures,
        )

    def test_rejects_split_config_writes_reintroduced_in_the_create_flow(self) -> None:
        for function in (
            "create_instance_transaction", "create_instance_in_pool", "materialize_new_instance_files",
        ):
            for call in ("write_instance_config(config_path, input)?;", "materialize_module_support_files(context)?;"):
                with self.subTest(function=function, call=call):
                    mutated = self.mutate_instance_function(function, lambda body: call + "\n" + body)
                    self.assertIn(
                        "dontstarve: create_instance_in_pool must not split support-file and instance.json writes",
                        verifier.validate_dst_native_settings_contract(
                            verifier.ROOT, storage_instances_source=mutated
                        ),
                    )

    def test_rejects_deleting_a_master_native_inventory_item(self) -> None:
        marker = '("master_world_size", "world_size", "default"),'
        self.assertIn(marker, self.storage_templates_source)
        mutated = self.storage_templates_source.replace(marker, "", 1)

        failures = verifier.validate_dst_native_settings_contract(
            verifier.ROOT,
            storage_templates_source=mutated,
        )

        self.assertTrue(
            any("renderer Master native inventory" in failure for failure in failures),
            failures,
        )

    def test_rejects_missing_master_event_in_caves_inheritance(self) -> None:
        marker = 'const DST_CAVES_INHERITED_OVERRIDE_KEYS: &[&str] = &['
        inherited = self.storage_templates_source.split(marker, 1)[1].split("];", 1)[0]
        self.assertIn('"year_of_the_snake",', inherited)
        mutated = self.storage_templates_source.replace(inherited, inherited.replace('"year_of_the_snake",', "", 1), 1)
        failures = verifier.validate_dst_native_settings_contract(
            verifier.ROOT, storage_templates_source=mutated
        )
        self.assertTrue(any("Caves inherited" in failure for failure in failures), failures)

    def test_rejects_surface_generation_key_in_caves_inheritance(self) -> None:
        marker = 'const DST_CAVES_INHERITED_OVERRIDE_KEYS: &[&str] = &['
        self.assertIn(marker, self.storage_templates_source)
        mutated = self.storage_templates_source.replace(marker, marker + '"world_size",', 1)
        failures = verifier.validate_dst_native_settings_contract(
            verifier.ROOT, storage_templates_source=mutated
        )
        self.assertTrue(any("Caves inherited" in failure for failure in failures), failures)

    def test_rejects_deleting_the_launch_args_token_resolver(self) -> None:
        marker = (
            '"launch_args" => Some(render_dontstarve_launch_args(context.settings).join("\\n")),'
        )
        self.assertIn(marker, self.runtime_launch_source)
        mutated = self.runtime_launch_source.replace(marker, "", 1)

        failures = verifier.validate_dst_native_settings_contract(
            verifier.ROOT,
            runtime_launch_source=mutated,
        )

        self.assertIn(
            "dontstarve: runtime token dontstarve.launch_args is missing its argv resolver",
            failures,
        )


class ScumNativeSettingsCoverageVerifierTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.module_source = (
            verifier.ROOT / "apps" / "desktop" / "src" / "views" / "settings" /
            "modules" / "scum.ts"
        ).read_text(encoding="utf-8-sig")
        cls.renderer_source = (
            verifier.ROOT / "crates" / "app-storage" / "src" /
            "templates_render_scum.rs"
        ).read_text(encoding="utf-8-sig")

    def test_accepts_current_scum_structured_contract(self) -> None:
        self.assertEqual(verifier.validate_scum_native_settings_contract(verifier.ROOT), [])

    def test_rejects_deleting_a_native_section_mapping(self) -> None:
        mutated = self.module_source.replace("id: section.toLowerCase()", "id: removedSection", 1)
        failures = verifier.validate_scum_native_settings_contract(
            verifier.ROOT, module_source=mutated
        )
        self.assertTrue(any("all six native" in failure for failure in failures), failures)

    def test_rejects_deleting_a_structured_renderer_use(self) -> None:
        mutated = self.renderer_source.replace('"server_world"', '"removed_server_world"', 1)
        failures = verifier.validate_scum_native_settings_contract(
            verifier.ROOT, renderer_source=mutated
        )
        self.assertTrue(any("server_world" in failure for failure in failures), failures)

    def test_rejects_deleting_a_native_template_token(self) -> None:
        templates = {
            name: (verifier.ROOT / "modules" / "scum" / "templates" / name).read_text(
                encoding="utf-8-sig"
            )
            for name in verifier.SCUM_TEMPLATE_CONTRACT
        }
        templates["ServerSettings.ini.hbs"] = ""
        failures = verifier.validate_scum_native_settings_contract(
            verifier.ROOT, template_sources=templates
        )
        self.assertTrue(any("server_settings_ini" in failure for failure in failures), failures)


class WindroseMaterializedSettingsCoverageVerifierTests(unittest.TestCase):
    world_keys = {
        "world_name",
        "world_preset_type",
        "coop_quests",
        "easy_explore",
        "mob_health_multiplier",
        "mob_damage_multiplier",
        "ship_health_multiplier",
        "ship_damage_multiplier",
        "boarding_difficulty_multiplier",
        "coop_stats_correction_modifier",
        "coop_ship_stats_correction_modifier",
        "combat_difficulty",
    }

    def test_accepts_world_fields_written_by_the_native_document_renderer(self) -> None:
        materialized = verifier.materialized_settings(
            "windrose", verifier.read_schema_properties("windrose")
        )

        self.assertTrue(self.world_keys <= materialized, materialized)

    def test_rejects_deleting_a_world_field_from_the_native_document_renderer(self) -> None:
        source = verifier.read_app_storage_templates_rs()
        marker = 'required_setting_string(settings, "world_name")?'
        self.assertIn(marker, source)
        original_cache = verifier._APP_STORAGE_TEMPLATES_TEXT
        verifier._APP_STORAGE_TEMPLATES_TEXT = source.replace(
            marker,
            'required_setting_string(settings, "removed_world_name")?',
            1,
        )
        try:
            materialized = verifier.materialized_settings(
                "windrose", verifier.read_schema_properties("windrose")
            )
        finally:
            verifier._APP_STORAGE_TEMPLATES_TEXT = original_cache

        self.assertNotIn("world_name", materialized)


class SatisfactoryMaterializedSettingsCoverageVerifierTests(unittest.TestCase):
    def test_accepts_settings_merged_into_the_native_integer_map(self) -> None:
        materialized = verifier.materialized_settings(
            "satisfactory", verifier.read_schema_properties("satisfactory")
        )
        self.assertEqual(materialized, {
            "auto_pause_when_empty", "network_quality", "send_gameplay_data",
            "weather_preset",
        })

    def test_rejects_a_setting_removed_from_the_native_map(self) -> None:
        source = verifier.read_app_storage_templates_rs()
        marker = '("auto_pause_when_empty", "FG.DSAutoPause"),'
        self.assertIn(marker, source)
        with patch.object(verifier, "_APP_STORAGE_TEMPLATES_TEXT", source.replace(marker, "", 1)):
            materialized = verifier.materialized_settings(
                "satisfactory", verifier.read_schema_properties("satisfactory")
            )
        self.assertNotIn("auto_pause_when_empty", materialized)
        self.assertEqual(materialized, {
            "network_quality", "send_gameplay_data", "weather_preset",
        })

    def test_rejects_weather_validation_without_a_native_map_entry(self) -> None:
        source = verifier.read_app_storage_templates_rs()
        marker = '("weather_preset", "FG.WeatherPreset"),'
        self.assertIn(marker, source)
        for replacement in ("", f"// {marker}"):
            with self.subTest(replacement=replacement):
                mutated = source.replace(marker, replacement, 1)
                self.assertIn('"weather_preset" => value.as_i64()', mutated)
                with patch.object(verifier, "_APP_STORAGE_TEMPLATES_TEXT", mutated):
                    materialized = verifier.materialized_settings(
                        "satisfactory", verifier.read_schema_properties("satisfactory")
                    )
                self.assertNotIn("weather_preset", materialized)
                self.assertEqual(materialized, {
                    "auto_pause_when_empty", "network_quality", "send_gameplay_data",
                })


class PalworldRestActionContractVerifierTests(unittest.TestCase):
    def make_module(self) -> dict:
        import copy
        module = copy.deepcopy(verifier.read_module_toml("palworld"))
        templates = {"broadcast": "announce {{target}}", "show_players": "players", "kick_player": "kick {{target}}", "ban_player": "ban {{target}}", "unban_player": "unban {{target}}", "save_world": "save"}
        for action in module["runtime"]["player_actions"]:
            action.update(transport="palworld_rest", port_name="rest_api", password_setting_key="admin_password", enabled_setting_key="rest_api_enabled", command_template=templates[action["id"]])
        return module

    def test_accepts_the_authenticated_instance_rest_contract(self) -> None:
        self.assertEqual(verifier.validate_runtime_player_actions("palworld", self.make_module(), verifier.read_rendered_surfaces("palworld")), [])

    def test_rejects_an_unbound_rest_endpoint_or_unknown_operation(self) -> None:
        for key, value in [("port_name", "rcon"), ("password_setting_key", "server_password"), ("command_template", "delete {{target}}")]:
            module = self.make_module()
            action = next(action for action in module["runtime"]["player_actions"] if action["id"] == "kick_player")
            action[key] = value
            failures = verifier.validate_runtime_player_actions("palworld", module, verifier.read_rendered_surfaces("palworld"))
            self.assertTrue(any("REST" in failure for failure in failures), failures)


if __name__ == "__main__":
    unittest.main()
