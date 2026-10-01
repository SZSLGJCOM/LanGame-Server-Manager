"""Recognize source literals without treating code references as credentials."""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path


SOURCE_CODE_SUFFIXES = {
    ".cjs",
    ".cpp",
    ".go",
    ".h",
    ".hpp",
    ".js",
    ".jsx",
    ".mjs",
    ".py",
    ".rs",
    ".ts",
    ".tsx",
}
JAVASCRIPT_SOURCE_SUFFIXES = {".cjs", ".js", ".jsx", ".mjs", ".ts", ".tsx"}
CPP_SOURCE_SUFFIXES = {".cpp", ".h", ".hpp"}
MULTILINE_LITERAL_SUFFIXES = (
    JAVASCRIPT_SOURCE_SUFFIXES | CPP_SOURCE_SUFFIXES | {".go", ".py", ".rs"}
)

RUST_RAW_STRING_START = re.compile(r'(?:br|r)(?P<hashes>#{0,255})"')
CPP_RAW_STRING_START = re.compile(
    r'(?:u8|u|U|L)?R"(?P<delimiter>[^ ()\\\t\r\n]{0,16})\('
)
C_LIKE_CHAR_LITERAL = re.compile(
    r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\}|.)|[^\\'\r\n])'"
)


@dataclass
class SourceLiteralState:
    quote: str | None = None
    raw_closer: str | None = None
    block_comment_depth: int = 0
    escaped: bool = False


def _inside_quoted_literal(line: str, index: int) -> bool:
    for quote in ('"', "'", "`"):
        quote_count = 0
        escaped = False
        for character in line[:index]:
            if character == "\\" and not escaped:
                escaped = True
                continue
            if character == quote and not escaped:
                quote_count += 1
            escaped = False
        if quote_count % 2 == 1:
            return True
    return False


def source_literal_mask(
    path: Path, line: str, state: SourceLiteralState
) -> list[bool]:
    suffix = path.suffix.casefold()
    mask = [False] * len(line)
    if suffix not in MULTILINE_LITERAL_SUFFIXES:
        return mask

    is_javascript = suffix in JAVASCRIPT_SOURCE_SUFFIXES
    is_cpp = suffix in CPP_SOURCE_SUFFIXES
    is_go = suffix == ".go"
    is_python = suffix == ".py"
    is_rust = suffix == ".rs"
    index = 0
    while index < len(line):
        if state.raw_closer is not None:
            if line.startswith(state.raw_closer, index):
                end = index + len(state.raw_closer)
                mask[index:end] = [True] * len(state.raw_closer)
                state.raw_closer = None
                index = end
            else:
                mask[index] = True
                index += 1
            continue

        if state.quote is not None:
            mask[index] = True
            character = line[index]
            if state.escaped:
                state.escaped = False
            elif character == "\\":
                state.escaped = True
            elif character == state.quote:
                state.quote = None
            index += 1
            continue

        if state.block_comment_depth:
            if line.startswith("*/", index):
                mask[index : index + 2] = [True, True]
                state.block_comment_depth -= 1
                index += 2
            elif is_rust and line.startswith("/*", index):
                mask[index : index + 2] = [True, True]
                state.block_comment_depth += 1
                index += 2
            else:
                mask[index] = True
                index += 1
            continue

        if is_python and line[index] == "#":
            mask[index:] = [True] * (len(line) - index)
            break
        if not is_python and line.startswith("//", index):
            mask[index:] = [True] * (len(line) - index)
            break
        if not is_python and line.startswith("/*", index):
            mask[index : index + 2] = [True, True]
            state.block_comment_depth = 1
            index += 2
            continue

        if is_javascript:
            if line[index] in ('"', "'", "`"):
                state.quote = line[index]
                mask[index] = True
            index += 1
            continue

        if is_python and line.startswith(('"""', "'''"), index):
            state.raw_closer = line[index : index + 3]
            mask[index : index + 3] = [True, True, True]
            index += 3
            continue

        raw_start = RUST_RAW_STRING_START.match(line, index) if is_rust else None
        if raw_start is not None and (
            index == 0
            or not (line[index - 1].isalnum() or line[index - 1] == "_")
        ):
            end = raw_start.end()
            mask[index:end] = [True] * (end - index)
            state.raw_closer = '"' + raw_start.group("hashes")
            index = end
            continue

        cpp_raw_start = CPP_RAW_STRING_START.match(line, index) if is_cpp else None
        if cpp_raw_start is not None and (
            index == 0
            or not (line[index - 1].isalnum() or line[index - 1] == "_")
        ):
            end = cpp_raw_start.end()
            mask[index:end] = [True] * (end - index)
            state.raw_closer = ')' + cpp_raw_start.group("delimiter") + '"'
            index = end
            continue

        if (is_cpp or is_go or is_rust) and line[index] == "'":
            char_literal = C_LIKE_CHAR_LITERAL.match(line, index)
            if char_literal is not None:
                index = char_literal.end()
                continue

        if is_go and line[index] in ('"', "`"):
            state.quote = line[index]
            mask[index] = True
        elif is_python and line[index] in ('"', "'"):
            state.quote = line[index]
            mask[index] = True
        elif (is_cpp or is_rust) and line[index] == '"':
            state.quote = '"'
            mask[index] = True
        index += 1

    single_line_quote = (
        (is_javascript and state.quote in ('"', "'"))
        or (is_go and state.quote == '"')
        or is_python
        or is_cpp
    )
    if single_line_quote and not state.escaped:
        state.quote = None
    state.escaped = False
    return mask


def match_is_literal(
    path: Path, line: str, value_index: int, literal_mask: list[bool]
) -> bool:
    suffix = path.suffix.casefold()
    if suffix not in SOURCE_CODE_SUFFIXES:
        return True
    if suffix in MULTILINE_LITERAL_SUFFIXES:
        return value_index < len(literal_mask) and literal_mask[value_index]
    return _inside_quoted_literal(line, value_index)
