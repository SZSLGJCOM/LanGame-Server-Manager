from __future__ import annotations

import re

from verify_module_setting_coverage_typescript import (
    extract_ts_balanced_block,
    extract_ts_braced_block,
    extract_ts_bracket_block,
    ts_executable_source,
)


def ts_exported_function_body(source: str, name: str) -> str:
    code = ts_executable_source(source)
    declaration = re.search(rf"\bexport\s+function\s+{re.escape(name)}\s*\(", code)
    if declaration is None:
        return ""
    parameters = extract_ts_balanced_block(code, declaration.end() - 1, "(", ")")
    if not parameters:
        return ""
    opening = code.find("{", declaration.end() - 1 + len(parameters))
    body = extract_ts_braced_block(code, opening)
    return source[opening:opening + len(body)] if body else ""


def validate_server_detail_tool_tabs(view_source: str, specs_source: str) -> list[str]:
    failures: list[str] = []
    view_code = ts_executable_source(view_source)
    view_body = ts_exported_function_body(view_source, "ServersView")
    body_code = ts_executable_source(view_body)
    builder_import = re.search(
        r'import\s*\{\s*buildServerDetailTabSpecs\s*\}\s*from\s*'
        r'["\']\./servers/server-detail-tab-specs["\']\s*;',
        view_source,
    )
    builder_call = re.search(
        r"\bconst\s+(\w+)\s*=\s*buildServerDetailTabSpecs\s*\(", body_code
    )
    call_prefix = body_code[1:builder_call.start()] if builder_call else ""
    if (
        builder_import is None
        or not view_code[builder_import.start():].startswith("import")
        or builder_call is None
        or call_prefix.count("{") != call_prefix.count("}")
        or re.search(
            rf"<ServerDetailTabs\b[^>]*\btabs\s*=\s*\{{\s*{re.escape(builder_call.group(1))}\s*\}}",
            body_code,
        ) is None
    ):
        failures.append(
            "frontend: ServersView must import and render the result of buildServerDetailTabSpecs"
        )

    specs_body = ts_exported_function_body(specs_source, "buildServerDetailTabSpecs")
    specs_code = ts_executable_source(specs_body)
    returned = re.search(r"\breturn\s*\[", specs_code)
    permanent_ids: set[str] = set()
    delimiters = (("{", "}"), ("(", ")"), ("[", "]"))
    if returned is not None:
        prefix = specs_code[1:returned.start()]
        array = extract_ts_bracket_block(specs_body, returned.end() - 1)
        suffix = specs_code[returned.end() - 1 + len(array):]
        # Only direct objects in the builder's final, unconditional return count.
        if (
            array
            and len(re.findall(r"\breturn\b", specs_code)) == 1
            and not re.search(r"\bif\b", prefix)
            and all(prefix.count(left) == prefix.count(right) for left, right in delimiters)
            and re.fullmatch(r"\s*;?\s*\}", suffix)
        ):
            array_code = ts_executable_source(array)
            cursor = 1
            while cursor < len(array) - 1:
                if array_code[cursor].isspace():
                    cursor += 1
                    continue
                entry = extract_ts_braced_block(array, cursor)
                if not entry:
                    permanent_ids.clear()
                    break
                entry_code = ts_executable_source(entry)
                for prop in re.finditer(r"\bid\s*:", entry_code):
                    prefix = entry_code[1:prop.start()]
                    if any(prefix.count(left) != prefix.count(right) for left, right in delimiters):
                        continue
                    value = re.match(r'id\s*:\s*["\']([^"\']+)["\']', entry[prop.start():])
                    if value:
                        permanent_ids.add(value.group(1))
                cursor += len(entry)
                while cursor < len(array) - 1 and array_code[cursor].isspace():
                    cursor += 1
                if cursor < len(array) - 1 and array_code[cursor] != ",":
                    permanent_ids.clear()
                    break
                cursor += 1

    for tab_id, component in (("gm", "GMToolsWorkbench"), ("players", "PlayerCenterWorkbench")):
        if tab_id not in permanent_ids:
            failures.append(f"frontend: server detail tab builder must permanently return the {tab_id} tab")
        selected_tab = rf'activeDetailTab\s*===\s*["\']{tab_id}["\']'
        guards = [
            (rf"{selected_tab}\s*&&\s*props\.selectedDetails", False),
            (rf"props\.selectedDetails\s*&&\s*{selected_tab}", False),
        ]
        if tab_id == "gm":
            guards.append((
                rf"props\.selectedDetails\s*&&\s*\(\s*{selected_tab}\s*\|\|\s*"
                r"retainedGmInstanceId\s*===\s*props\.selectedDetails\.summary\.id\s*\)",
                True,
            ))
        renders_workbench = False
        for guard, retained in guards:
            for branch in re.finditer(rf"\{{\s*{guard}\s*\?\s*\(", view_body):
                if body_code[branch.start()] != "{":
                    continue
                opening = branch.end() - 1
                rendered_code = extract_ts_balanced_block(body_code, opening, "(", ")")
                if retained:
                    # Retained operations may stay mounted only behind the GM visibility boundary.
                    rendered_source = view_body[opening:opening + len(rendered_code)]
                    visible_wrapper = re.fullmatch(
                        r'\(\s*<div\s+hidden=\{\s*activeDetailTab\s*!==\s*["\']gm["\']\s*\}>'
                        r'(?P<content>[\s\S]*)</div>\s*\)',
                        rendered_source,
                    )
                    if visible_wrapper is None:
                        continue
                    rendered_code = ts_executable_source(visible_wrapper.group("content"))
                if re.search(rf"<{component}\b", rendered_code):
                    renders_workbench = True
        if not renders_workbench:
            failures.append(f"frontend: ServersView must render {component} in the {tab_id} tab")
    return failures
