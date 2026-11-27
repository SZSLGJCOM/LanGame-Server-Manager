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
