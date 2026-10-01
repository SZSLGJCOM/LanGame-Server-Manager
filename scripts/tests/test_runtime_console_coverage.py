from __future__ import annotations

from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from scripts import verify_module_setting_coverage as verifier
from verify_module_setting_coverage_console import validate_runtime_console_boundary


class RuntimeConsoleCoverageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.view = verifier.read_frontend_file("views/servers/RuntimeSurfaceWorkbench.tsx")
        cls.transport = verifier.read_frontend_file("runtime-console-transport.ts")

    def changed(self, source: str, before: str, after: str) -> str:
        self.assertEqual(source.count(before), 1, before)
        return source.replace(before, after, 1)

    def check(self, *, view: str | None = None, transport: str | None = None) -> list[str]:
        return validate_runtime_console_boundary(
            self.view if view is None else view,
            self.transport if transport is None else transport,
        )

    def test_accepts_current_extracted_routes_and_selected_instance_binding(self) -> None:
        self.assertEqual(self.check(), [])

    def test_accepts_local_binding_renames_and_dispatch_whitespace(self) -> None:
        view = self.view.replace("commandRoute", "resolvedTransport").replace("runtimeCommandEnabled", "canSubmitCommand")
        view = self.changed(view, "const instanceId = props.details.summary.id;", "const submittedInstanceId = props.details.summary.id;")
        view = self.changed(view, "onSendRuntimeCommand(instanceId, command,", "onSendRuntimeCommand(\nsubmittedInstanceId,\ncommand,")
        self.assertEqual(self.check(view=view), [])

    def test_accepts_direct_selected_instance_identity(self) -> None:
        view = self.changed(self.view, "onSendRuntimeCommand(instanceId, command,", "onSendRuntimeCommand(props.details.summary.id, command,")
        self.assertEqual(self.check(view=view), [])

    def test_resolver_must_be_imported_and_called_with_current_settings(self) -> None:
        for before, after in (
            ('import { resolveRuntimeConsoleTransport } from "../../runtime-console-transport";', '/* import { resolveRuntimeConsoleTransport } from "../../runtime-console-transport"; */'),
            ('from "../../runtime-console-transport"', 'from "../../unused-transport"'),
            ("useMemo(() => resolveRuntimeConsoleTransport(", "useMemo(() => unusedTransport("),
            ("props.moduleDetails ?? null, props.details.settings_json", "props.moduleDetails ?? null, staleSettings"),
        ):
            with self.subTest(after=after):
                self.assertTrue(self.check(view=self.changed(self.view, before, after)))

    def test_unused_component_cannot_supply_the_dispatch_boundary(self) -> None:
        view = self.changed(self.view, "export function RuntimeSurfaceWorkbench(", "export function UnusedRuntimeSurfaceWorkbench(")
        view += "\nexport function RuntimeSurfaceWorkbench() { return null; }\n"
        self.assertTrue(self.check(view=view))

    def test_availability_cannot_drop_or_bypass_any_prerequisite(self) -> None:
        for before, after in (
            ("instanceRunning && !selectedTarget?.disabled &&", "!selectedTarget?.disabled &&"),
            ("!selectedTarget?.disabled && commandRoute.available", "commandRoute.available"),
            ("&& commandRoute.available &&", "&& true &&"),
            ("&& commandRoute.available &&", "|| commandRoute.available ||"),
            ("&& Boolean(props.onSendRuntimeCommand)", "|| true"),
        ):
            with self.subTest(after=after):
                self.assertTrue(self.check(view=self.changed(self.view, before, after)))

    def test_submit_guard_cannot_drop_a_rejection_or_stop_returning(self) -> None:
        guard = "readOnly || !command || commandPending || !selectedTarget || !runtimeCommandEnabled || !props.onSendRuntimeCommand"
        for term in guard.split(" || "):
            with self.subTest(term=term):
                changed_guard = " || ".join(part for part in guard.split(" || ") if part != term)
                self.assertTrue(self.check(view=self.changed(self.view, guard, changed_guard)))
        before = f"if ({guard}) {{\n      return;\n    }}"
        for after in (f"/* {before} */", f"if ({guard}) {{\n      logUnavailable();\n    }}", f"if (false) {{ {before} }}"):
            with self.subTest(after=after):
                self.assertTrue(self.check(view=self.changed(self.view, before, after)))

    def test_dispatch_requires_selected_instance_process_and_resolved_options(self) -> None:
        for before, after in (
            ("const instanceId = props.details.summary.id;", "const instanceId = unrelatedInstanceId;"),
            ("await props.onSendRuntimeCommand(instanceId, command,", "props.onSendRuntimeCommand(instanceId, command,"),
            ("onSendRuntimeCommand(instanceId, command, selectedTarget.processKey ?? null,", "onSendRuntimeCommand(instanceId, command, null,"),
            ("        commandRoute.options);", "        undefined);"),
            ("        commandRoute.options);", "        { transport: 'stdin' });"),
            ("const command = runtimeCommandDraft.trim();", "props.onSendRuntimeCommand(props.details.summary.id, runtimeCommandDraft);\n    const command = runtimeCommandDraft.trim();"),
        ):
            with self.subTest(after=after):
                self.assertTrue(self.check(view=self.changed(self.view, before, after)))

    def test_form_cannot_bypass_the_guarded_handler(self) -> None:
        view = self.changed(self.view, "void handleRuntimeCommandSubmit();", "void props.onSendRuntimeCommand?.(props.details.summary.id, runtimeCommandDraft);")
        self.assertTrue(self.check(view=view))

    def test_player_action_metadata_stays_out_of_console_and_routes(self) -> None:
        for name in ("runtimeActionId", "runtimeActionTarget", "runtimeActionRole"):
            for target in ("view", "transport"):
                with self.subTest(name=name, target=target):
                    source = getattr(self, target) + f'\nconst leaked = {{ {name}: "action" }};\n'
                    self.assertTrue(self.check(**{target: source}))
        self.assertEqual(self.check(view=self.view + '\n// runtimeActionId: "example"\nconst example = "runtimeActionRole: player";'), [])

    def test_resolver_requires_live_module_declarations_and_settings_evaluation(self) -> None:
        for before, after in (
            ("module.runtime.player_actions ?? []", "[]"),
            ("module.runtime.shutdown?.commands ?? []", "[]"),
            ("uniqueRoutes([...actions, ...shutdown])", "uniqueRoutes(actions)"),
            ("JSON.parse(settingsJson)", "JSON.parse('{}')"),
            ("!Array.isArray(parsed)", "true"),
            ("issue: routeIssue(route, settings)", "issue: null"),
            ("evaluated.filter(candidate => !candidate.issue)", "evaluated"),
        ):
            with self.subTest(after=after):
                self.assertTrue(self.check(transport=self.changed(self.transport, before, after)))

    def test_unknown_ambiguous_and_valid_route_results_cannot_be_swapped(self) -> None:
        for before, after in (
            ("if (!module) return {\n    available: false", "if (!module) return {\n    available: true"),
            ("if (available.length > 1) return {\n    available: false", "if (available.length > 1) return {\n    available: true"),
            ("options: available[0].route", "options: routes[0]"),
            ("if (available.length === 1) return", "if (available.length >= 1) return"),
        ):
            with self.subTest(after=after):
                self.assertTrue(self.check(transport=self.changed(self.transport, before, after)))

    def test_resolver_cannot_be_replaced_by_an_unused_or_commented_copy(self) -> None:
        for source in (
            f"/* {self.transport} */",
            self.changed(self.transport, "export function resolveRuntimeConsoleTransport(", "export function unusedTransport("),
        ):
            self.assertTrue(self.check(transport=source))

    def test_declared_protocol_and_credentials_cannot_be_discarded(self) -> None:
        for before, after in (
            ("const route = routeFor(declaration);", "const route = nativeOptions;"),
            ("if (route) routes.set", "if (false) routes.set"),
            ("return [...routes.values()]", "return []"),
            ("normalized(declaration.transport)?.toLowerCase()", "undefined"),
            ("return {\n    transport,", 'return {\n    transport: "stdin",'),
            ("normalized(declaration.port_name)", "undefined"),
            ("normalized(declaration.password_setting_key)", "undefined"),
            ("normalized(declaration.enabled_setting_key)", "undefined"),
            ('if (transport !== "source_rcon" && transport !== "websocket_rcon" && transport !== "telnet") return null;', '/* if (transport !== "source_rcon" && transport !== "websocket_rcon" && transport !== "telnet") return null; */'),
            ("if (!settings) return", "if (false) return"),
            ("configuredBoolean(settings[route.enabledSettingKey])", "true"),
            ("if (!enabled) return", "if (false) return"),
            ("!password.trim()", "false"),
        ):
            with self.subTest(after=after):
                self.assertTrue(self.check(transport=self.changed(self.transport, before, after)))

    def test_frontend_coverage_calls_the_route_validator(self) -> None:
        original = verifier.read_frontend_file
        source = self.changed(self.transport, "issue: routeIssue(route, settings)", "issue: null")
        with patch.object(verifier, "read_frontend_file", lambda path: source if path == "runtime-console-transport.ts" else original(path)):
            self.assertIn(
                "frontend: runtime console resolver must evaluate declared module routes against instance settings",
                verifier.validate_frontend_coverage(),
            )


if __name__ == "__main__":
    unittest.main()
