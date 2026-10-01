from __future__ import annotations

import re
from pathlib import Path

from source_literal_scan import SourceLiteralState, source_literal_mask


def ts_executable_source(source: str) -> str:
    state = SourceLiteralState()
    code = []
    for line in source.splitlines(keepends=True):
        mask = source_literal_mask(Path("source.ts"), line, state)
        code.append("".join(" " if hidden else character for character, hidden in zip(line, mask)))
    return "".join(code)


def extract_ts_balanced_block(
    source: str, open_index: int, open_char: str, close_char: str
) -> str:
    if open_index < 0 or open_index >= len(source) or source[open_index] != open_char:
        return ""

    depth = 0
    quote: str | None = None
    escaped = False
    line_comment = False
    block_comment = False

    for index in range(open_index, len(source)):
        char = source[index]
        next_char = source[index + 1] if index + 1 < len(source) else ""

        if line_comment:
            if char == "\n":
                line_comment = False
            continue

        if block_comment:
            if char == "*" and next_char == "/":
                block_comment = False
            continue

        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
            continue

        if char == "/" and next_char == "/":
            line_comment = True
            continue

        if char == "/" and next_char == "*":
            block_comment = True
            continue

        if char in ("'", '"', "`"):
            quote = char
            continue

        if char == open_char:
            depth += 1
        elif char == close_char:
            depth -= 1
            if depth == 0:
                return source[open_index : index + 1]

    return ""


def extract_ts_braced_block(source: str, open_brace_index: int) -> str:
    return extract_ts_balanced_block(source, open_brace_index, "{", "}")


def extract_ts_bracket_block(source: str, open_bracket_index: int) -> str:
    return extract_ts_balanced_block(source, open_bracket_index, "[", "]")


def extract_ts_object_block(source: str, marker: str) -> str:
    start = source.find(marker)
    if start < 0:
        return ""
    end = source.find("\n};", start)
    return source[start:end] if end >= 0 else source[start:]


def settings_definition_object_block(source: str, export_match: re.Match[str]) -> str:
    open_brace_index = source.find("{", export_match.end())
    return extract_ts_braced_block(source, open_brace_index)


def ts_object_keys(source: str, const_name: str) -> set[str]:
    block = extract_ts_object_block(source, f"const {const_name}")
    keys: set[str] = set()
    for line in block.splitlines():
        quoted = re.match(r'^\s*"((?:\\.|[^"\\])*)"\s*:', line)
        if quoted:
            keys.add(quoted.group(1).replace('\\"', '"').replace("\\\\", "\\"))
            continue
        identifier = re.match(r"^\s*([A-Za-z0-9_]+)\s*:", line)
        if identifier:
            keys.add(identifier.group(1))
    return keys


def ts_object_string_values(source: str) -> dict[str, str]:
    values: dict[str, str] = {}
    for line in source.splitlines():
        match = re.match(
            r'^\s*"((?:\\.|[^"\\])*)"\s*:\s*"((?:\\.|[^"\\])*)\s*(?:",\s*$|"\s*$|,\s*$)$',
            line,
        )
        if not match:
            continue
        key = match.group(1).replace('\\"', '"').replace("\\\\", "\\")
        value = match.group(2).replace('\\"', '"').replace("\\\\", "\\")
        values[key] = value
    return values


def ts_string_sequence(source: str, const_name: str) -> set[str]:
    marker = f"const {const_name}"
    start = source.find(marker)
    if start < 0:
        return set()
    line_end = source.find("\n", start)
    line = source[start:] if line_end < 0 else source[start:line_end]
    return set(re.findall(r'"((?:\\.|[^"\\])*)"', line))


def ts_object_has_property(object_block: str, property_name: str) -> bool:
    return (
        re.search(
            rf"(^|[{{,]\s*){re.escape(property_name)}\s*(?::|\()",
            object_block,
            re.MULTILINE,
        )
        is not None
    )


def ts_object_has_string_property(
    object_block: str, property_name: str, expected_value: str
) -> bool:
    return (
        re.search(
            rf"(^|[{{,]\s*){re.escape(property_name)}\s*:\s*[\"']{re.escape(expected_value)}[\"']",
            object_block,
            re.MULTILINE,
        )
        is not None
    )


def ts_string_literals(source: str) -> set[str]:
    return set(re.findall(r"[\"']([A-Za-z0-9_\-.]+)[\"']", source))


def ts_const_string_array(source: str, const_name: str) -> list[str]:
    match = re.search(
        rf"\b(?:export\s+)?const\s+{re.escape(const_name)}\s*=\s*\[", source
    )
    if match is None:
        return []
    open_index = source.find("[", match.end() - 1)
    array_block = extract_ts_bracket_block(source, open_index)
    return re.findall(r'["\']([A-Za-z0-9_\-.]+)["\']', array_block)


def ts_group_key_literal_entries(source: str) -> list[str]:
    keys: list[str] = []
    for match in re.finditer(r"\bkeys\s*:\s*\[([^\]]*)\]", source, re.DOTALL):
        block = match.group(1)
        keys.extend(re.findall(r"[\"']([A-Za-z0-9_\-.]+)[\"']", block))
        for const_name in re.findall(r"\.\.\.([A-Za-z0-9_]+)", block):
            keys.extend(ts_const_string_array(source, const_name))
    return keys


def ts_group_key_literals(source: str) -> set[str]:
    return set(ts_group_key_literal_entries(source))


def ts_section_ids_from_array_block(array_block: str) -> set[str]:
    return set(
        re.findall(
            r"(?:^|[{\[,]\s*)id\s*:\s*[\"']([A-Za-z0-9_-]+)[\"']",
            array_block,
            re.MULTILINE,
        )
    )


def module_settings_section_ids_from_source(source: str) -> set[str]:
    section_ids: set[str] = set()

    for match in re.finditer(
        r"\bconst\s+[A-Za-z0-9_]*SECTIONS\b[^=]*=\s*\[", source
    ):
        open_index = source.find("[", match.end() - 1)
        array_block = extract_ts_bracket_block(source, open_index)
        section_ids.update(ts_section_ids_from_array_block(array_block))

    for match in re.finditer(
        r"\bfunction\s+[A-Za-z0-9_]*Sections\s*\([^)]*\)[^{]*\{", source
    ):
        open_index = source.find("{", match.end() - 1)
        function_block = extract_ts_braced_block(source, open_index)
        for return_match in re.finditer(r"\breturn\s*\[", function_block):
            array_open_index = function_block.find("[", return_match.end() - 1)
            array_block = extract_ts_bracket_block(function_block, array_open_index)
            section_ids.update(ts_section_ids_from_array_block(array_block))
        for array_match in re.finditer(
            r"\b(?:const|let)\s+[A-Za-z0-9_]+[^=]*=\s*\[", function_block
        ):
            array_open_index = function_block.find("[", array_match.end() - 1)
            array_block = extract_ts_bracket_block(function_block, array_open_index)
            section_ids.update(ts_section_ids_from_array_block(array_block))

    for match in re.finditer(r"\bgetSections\s*:\s*\([^)]*\)\s*=>\s*\[", source):
        open_index = source.find("[", match.end() - 1)
        array_block = extract_ts_bracket_block(source, open_index)
        section_ids.update(ts_section_ids_from_array_block(array_block))

    return section_ids
