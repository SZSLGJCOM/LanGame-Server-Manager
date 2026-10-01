from __future__ import annotations

import re

from verify_module_setting_coverage_typescript import (
    extract_ts_balanced_block,
    extract_ts_braced_block,
    ts_executable_source,
)


def _function_body(source: str, name: str, *, exported: bool = False) -> str:
    code = ts_executable_source(source)
    prefix = r"export\s+" if exported else ""
    match = re.search(rf"\b{prefix}(?:async\s+)?function\s+{re.escape(name)}\s*\(", code)
    if match is None:
        return ""
    parameters = extract_ts_balanced_block(code, match.end() - 1, "(", ")")
    opening = code.find("{", match.end() - 1 + len(parameters))
    body = extract_ts_braced_block(code, opening)
    return source[opening:opening + len(body)] if parameters and body else ""


def _live_match(source: str, pattern: str) -> re.Match[str] | None:
    code = ts_executable_source(source)
    for match in re.finditer(pattern, source):
        # Patterns start at executable syntax; comments and string examples cannot satisfy them.
        if code[match.start():match.start() + 1] == source[match.start():match.start() + 1]:
            return match
    return None


def _returned_branch(source: str, condition: str) -> str:
    match = _live_match(source, rf"if\s*\(\s*{condition}\s*\)\s*return\s*\{{")
    return extract_ts_braced_block(source, match.end() - 1) if match else ""


def _has_resolution(source: str, available: str, options: str) -> bool:
    code = ts_executable_source(source)
    return bool(
        re.search(rf"\bavailable\s*:\s*{available}\s*[,}}]", code)
        and re.search(rf"\boptions\s*:\s*{options}\s*[,}}]", code)
    )


def _at_body_level(code: str, position: int) -> bool:
    prefix = code[1:position]
    return all(prefix.count(left) == prefix.count(right) for left, right in (("{", "}"), ("(", ")"), ("[", "]")))


def validate_runtime_console_boundary(view_source: str, transport_source: str) -> list[str]:
    failures: list[str] = []
    view = _function_body(view_source, "RuntimeSurfaceWorkbench", exported=True)
    code = ts_executable_source(view)
    imported = _live_match(
        view_source,
        r'import\s*\{\s*resolveRuntimeConsoleTransport\s*\}\s*from\s*'
        r'["\']../../runtime-console-transport["\']\s*;',
    )
    resolved = re.search(
        r"\bconst\s+(\w+)\s*=\s*useMemo\s*\(\s*\(\s*\)\s*=>\s*"
        r"resolveRuntimeConsoleTransport\s*\(\s*props\.moduleDetails\s*\?\?\s*null\s*,\s*"
        r"props\.details\.settings_json\s*\)", code,
    )
    route = resolved.group(1) if resolved else "commandRoute"
    if imported is None or resolved is None or not _at_body_level(code, resolved.start()):
        failures.append("frontend: runtime console must resolve its module and instance settings through runtime-console-transport")

    # Find the availability expression, independently of the intervening hint declaration.
    enabled = next((match for match in re.finditer(r"\bconst\s+(\w+)\s*=\s*([^;]+);", code)
                    if re.search(rf"\b{re.escape(route)}\.available\b", match.group(2))), None)
    expected_terms = {"instanceRunning", "!selectedTarget?.disabled", f"{route}.available", "Boolean(props.onSendRuntimeCommand)"}
    terms = re.sub(r"\s+", "", enabled.group(2)).split("&&") if enabled else []
    if set(terms) != expected_terms or len(terms) != len(expected_terms) or not _at_body_level(code, enabled.start()):
        failures.append("frontend: runtime console availability must require a running selected target, an available route and a dispatch callback")
    enabled_name = enabled.group(1) if enabled else "runtimeCommandEnabled"

    submit = _function_body(view, "handleRuntimeCommandSubmit")
    submit_code = ts_executable_source(submit)
    guard = re.search(r"\bif\s*\(([^{};]+)\)\s*\{\s*return\s*;\s*\}", submit_code)
    rejected = {"readOnly", "!command", "commandPending", "!selectedTarget", f"!{enabled_name}", "!props.onSendRuntimeCommand"}
    guard_terms = re.sub(r"\s+", "", guard.group(1)).split("||") if guard else []
    dispatch = re.search(
        rf"\bawait\s+props\.onSendRuntimeCommand\s*\(\s*(\w+|props\.details\.summary\.id)\s*,\s*"
        rf"command\s*,\s*selectedTarget\.processKey\s*\?\?\s*null\s*,\s*{re.escape(route)}\.options\s*\)",
        submit_code,
    )
    if (
        guard is None or set(guard_terms) != rejected or len(guard_terms) != len(rejected)
        or not _at_body_level(submit_code, guard.start())
        or dispatch is None or guard.end() > dispatch.start()
        or len(re.findall(r"\bprops\.onSendRuntimeCommand\s*\(", code)) != 1
    ):
        failures.append("frontend: runtime console must reject unavailable submissions before awaiting one dispatch with the selected process and resolved route")
    if dispatch and dispatch.group(1) != "props.details.summary.id":
        instance = re.search(rf"\bconst\s+{re.escape(dispatch.group(1))}\s*=\s*props\.details\.summary\.id\s*;", submit_code)
        if instance is None or instance.end() > dispatch.start():
            failures.append("frontend: runtime console dispatch must use the selected instance identity")
    if re.search(r"\bonSubmit\s*=\s*\{\s*\(event\)\s*=>\s*\{\s*event\.preventDefault\(\);\s*void\s+handleRuntimeCommandSubmit\(\);", code) is None:
        failures.append("frontend: runtime console form must submit through the guarded command handler")

    for source in (view_source, transport_source):
        for marker in ("runtimeActionId", "runtimeActionTarget", "runtimeActionRole"):
            if re.search(rf"\b{marker}\s*:", ts_executable_source(source)):
                failures.append(f"frontend: runtime console must not dispatch structured player-action metadata {marker!r}; keep player actions in Player Center")

    resolver = _function_body(transport_source, "resolveRuntimeConsoleTransport", exported=True)
    resolver_code = ts_executable_source(resolver)
    declarations = (
        r"const\s+actions\s*=\s*module\.runtime\.player_actions\s*\?\?\s*\[\s*\]",
        r"const\s+shutdown\s*=\s*module\.runtime\.shutdown\?\.commands\s*\?\?\s*\[\s*\]",
        r"const\s+routes\s*=\s*uniqueRoutes\(\s*\[\s*\.\.\.actions\s*,\s*\.\.\.shutdown\s*\]\s*\)",
        r"const\s+parsed:\s*unknown\s*=\s*JSON\.parse\(settingsJson\)",
        r"!Array\.isArray\(parsed\)\)\s*settings\s*=\s*parsed\s+as\s+Record",
        r"const\s+evaluated\s*=\s*routes\.map\(route\s*=>\s*\(\{\s*route\s*,\s*issue:\s*routeIssue\(route,\s*settings\)\s*\}\)\)",
        r"const\s+available\s*=\s*evaluated\.filter\(candidate\s*=>\s*!candidate\.issue\)",
    )
    if any(re.search(pattern, resolver_code) is None for pattern in declarations):
        failures.append("frontend: runtime console resolver must evaluate declared module routes against instance settings")
    if not (
        _has_resolution(_returned_branch(resolver, r"!module"), "false", "undefined")
        and _has_resolution(_returned_branch(resolver, r"available\.length\s*>\s*1"), "false", "undefined")
        and _has_resolution(_returned_branch(resolver, r"available\.length\s*===\s*1"), "true", r"available\[0\]\.route")
    ):
        failures.append("frontend: runtime console resolver must reject unknown or ambiguous routes and return the one available route")

    unique = ts_executable_source(_function_body(transport_source, "uniqueRoutes"))
    route_source = _function_body(transport_source, "routeFor")
    route_code = ts_executable_source(route_source)
    if (
        re.search(r"const\s+route\s*=\s*routeFor\(declaration\)", unique) is None
        or re.search(r"if\s*\(route\)\s*routes\.set\(JSON\.stringify\(route\),\s*route\)", unique) is None
        or re.search(r"return\s*\[\s*\.\.\.routes\.values\(\)\s*\]", unique) is None
        or any(re.search(pattern, route_code) is None for pattern in (
            r"const\s+transport\s*=\s*normalized\(declaration\.transport\)\?\.toLowerCase\(\)",
            r"return\s*\{\s*transport\s*,",
            r"portName:\s*normalized\(declaration\.port_name\)",
            r"passwordSettingKey:\s*normalized\(declaration\.password_setting_key\)",
            r"enabledSettingKey:\s*normalized\(declaration\.enabled_setting_key\)",
        ))
        or _live_match(route_source, r'if\s*\(transport\s*!==\s*"source_rcon"\s*&&\s*transport\s*!==\s*"websocket_rcon"\s*&&\s*transport\s*!==\s*"telnet"\)\s*return\s+null;') is None
    ):
        failures.append("frontend: runtime console routes must retain declared transport options and reject unsupported remote protocols")
    issue = ts_executable_source(_function_body(transport_source, "routeIssue"))
    if any(re.search(pattern, issue) is None for pattern in (
        r"if\s*\(!settings\)\s*return\s*\{",
        r"if\s*\(route\.enabledSettingKey\)\s*\{",
        r"const\s+enabled\s*=\s*configuredBoolean\(settings\[route\.enabledSettingKey\]\)",
        r"if\s*\(!enabled\)\s*return\s*\{",
        r"const\s+password\s*=\s*settings\[route\.passwordSettingKey\s*\?\?",
        r"\|\|\s*!password\.trim\(\)\)\)\s*return\s*\{",
    )):
        failures.append("frontend: runtime console remote routes must check settings, enable flags and passwords")
    return failures
