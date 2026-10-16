"""Recognize source literals without treating code references as credentials."""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path


SOURCE_CODE_SUFFIXES = {
    ".cjs",
    ".cpp",
    ".cs",
    ".go",
    ".h",
    ".hpp",
    ".js",
    ".jsx",
    ".mjs",
    ".py",
    ".ps1",
    ".lua",
    ".rs",
    ".ts",
    ".tsx",
}
JAVASCRIPT_SOURCE_SUFFIXES = {".cjs", ".js", ".jsx", ".mjs", ".ts", ".tsx"}
CPP_SOURCE_SUFFIXES = {".cpp", ".h", ".hpp"}
MULTILINE_LITERAL_SUFFIXES = (
    JAVASCRIPT_SOURCE_SUFFIXES | CPP_SOURCE_SUFFIXES | {".go", ".py", ".rs", ".cs", ".lua", ".ps1"}
)

RUST_RAW_STRING_START = re.compile(r'(?:br|r)(?P<hashes>#{0,255})"')
CPP_RAW_STRING_START = re.compile(
    r'(?:u8|u|U|L)?R"(?P<delimiter>[^ ()\\\t\r\n]{0,16})\('
)
C_LIKE_CHAR_LITERAL = re.compile(
    r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\}|.)|[^\\'\r\n])'"
)
LUA_LONG_STRING_START = re.compile(r"\[(?P<equals>=*)\[")
CSHARP_STRING_START = re.compile(r'(?:\$@|@\$|@|\$)?"')
POWERSHELL_VARIABLE = re.compile(r"\$(?:\{[^}\r\n]*\}|(?:[\w]+:)?[\w?]+)")


@dataclass
class SourceLiteralState:
    quote: str | None = None
    raw_closer: str | None = None
    block_comment_depth: int = 0
    escaped: bool = False
    verbatim: bool = False
    interpolated: bool = False
    interpolation_depth: int = 0
    interpolation_quote: str | None = None
    interpolation_escaped: bool = False
    raw_line_start: bool = False
    raw_interpolated: bool = False


def _control_source_literal_mask(
    path: Path, line: str, state: SourceLiteralState, scan_line: list[str] | None
) -> list[bool]:
    """Recognize the control adapters' C# 5, Lua and PowerShell strings.

    Comments remain scanned. PowerShell bare arguments remain literal candidates;
    variable references are expressions even inside expandable strings.
    """
    is_csharp = path.suffix.casefold() == ".cs"
    is_lua = path.suffix.casefold() == ".lua"
    is_powershell = path.suffix.casefold() == ".ps1"
    mask = [is_powershell] * len(line)
    index = 0
    while index < len(line):
        if state.raw_closer is not None:
            if line.startswith(state.raw_closer, index) and (
                not state.raw_line_start or index == 0
            ):
                end = index + len(state.raw_closer)
                mask[index:end] = [True] * (end - index)
                state.raw_closer = None
                state.raw_line_start = False
                state.raw_interpolated = False
                index = end
            else:
                mask[index] = True
                if state.raw_interpolated:
                    if state.escaped:
                        state.escaped = False
                    elif line[index] == "`":
                        state.escaped = True
                        if scan_line is not None:
                            scan_line[index] = "\\"
                    elif line[index] == "$":
                        mask[index] = False
                index += 1
            continue

        if state.interpolation_depth:
            character = line[index]
            if state.interpolation_quote is not None:
                mask[index] = True
                if state.interpolation_escaped:
                    state.interpolation_escaped = False
                elif character == ("`" if is_powershell else "\\"):
                    state.interpolation_escaped = True
                elif character == state.interpolation_quote:
                    state.interpolation_quote = None
            else:
                if (is_powershell and character == "#") or (is_csharp and line.startswith("//", index)):
                    mask[index:] = [True] * (len(line) - index)
                    break
                if (is_powershell and line.startswith("<#", index)) or (is_csharp and line.startswith("/*", index)):
                    state.raw_closer = "#>" if is_powershell else "*/"
                    mask[index:index + 2] = [True, True]
                    index += 2
                    continue
                mask[index] = False
                if character in ('"', "'"):
                    state.interpolation_quote = character
                elif character == ("(" if is_powershell else "{"):
                    state.interpolation_depth += 1
                elif character == (")" if is_powershell else "}"):
                    state.interpolation_depth -= 1
            index += 1
            continue

        if state.quote is not None:
            character = line[index]
            mask[index] = True
            if state.escaped:
                state.escaped = False
            elif state.verbatim and line.startswith(state.quote * 2, index):
                mask[index:index + 2] = [True, True]
                if scan_line is not None:
                    scan_line[index] = "\\"
                index += 2
                continue
            elif not state.verbatim and character == ("`" if is_powershell else "\\"):
                state.escaped = True
                if scan_line is not None and is_powershell:
                    scan_line[index] = "\\"
            elif character == state.quote:
                state.quote = None
            elif state.interpolated and is_csharp and character == "{":
                if line.startswith("{{", index):
                    mask[index:index + 2] = [True, True]
                    index += 2
                    continue
                mask[index] = False
                state.interpolation_depth = 1
            elif state.interpolated and is_powershell and character == "$":
                mask[index] = False
                if line.startswith("$(", index):
                    mask[index + 1] = False
                    state.interpolation_depth = 1
                    index += 2
                    continue
            index += 1
            continue

        if is_lua and line.startswith("--", index):
            long_start = LUA_LONG_STRING_START.match(line, index + 2)
            if long_start is None:
                mask[index:] = [True] * (len(line) - index)
                break
            state.raw_closer = "]" + long_start.group("equals") + "]"
            end = long_start.end()
            mask[index:end] = [True] * (end - index)
            index = end
            continue
        if is_powershell and line[index] == "#" or is_csharp and line.startswith("//", index):
            mask[index:] = [True] * (len(line) - index)
            break
        if is_powershell and line.startswith("<#", index) or is_csharp and line.startswith("/*", index):
            state.raw_closer = "#>" if is_powershell else "*/"
            mask[index:index + 2] = [True, True]
            index += 2
            continue

        if is_lua:
            long_start = LUA_LONG_STRING_START.match(line, index)
            if long_start is not None:
                state.raw_closer = "]" + long_start.group("equals") + "]"
                end = long_start.end()
                mask[index:end] = [True] * (end - index)
                index = end
                continue
        if is_csharp:
            string_start = CSHARP_STRING_START.match(line, index)
            if string_start is not None:
                end = string_start.end()
                prefix = line[index:end - 1]
                state.quote = '"'
                state.verbatim = "@" in prefix
                state.interpolated = "$" in prefix
                mask[index:end] = [True] * (end - index)
                if scan_line is not None:
                    scan_line[index:end - 1] = [" "] * len(prefix)
                index = end
                continue
            if line[index] == "'":
                char_literal = C_LIKE_CHAR_LITERAL.match(line, index)
                if char_literal is not None:
                    index = char_literal.end()
                    continue
        if is_powershell:
            if line[index:] in ("@'", '@"') or (
                line[index:index + 2] in ("@'", '@"')
                and not line[index + 2:].strip()
            ):
                state.raw_closer = line[index + 1] + "@"
                state.raw_line_start = True
                state.raw_interpolated = line[index + 1] == '"'
                index = len(line)
                continue
            if line.startswith("$(", index):
                mask[index:index + 2] = [False, False]
                state.interpolation_depth = 1
                index += 2
                continue
            variable = POWERSHELL_VARIABLE.match(line, index)
            if variable is not None:
                mask[index:variable.end()] = [False] * (variable.end() - index)
                index = variable.end()
                continue
        if (is_lua or is_powershell) and line[index] in ('"', "'"):
            state.quote = line[index]
            state.verbatim = is_powershell and state.quote == "'"
            state.interpolated = is_powershell and state.quote == '"'
            mask[index] = True
        index += 1

    if (is_csharp or is_lua) and state.quote is not None and not state.verbatim and not state.escaped and not state.interpolation_depth:
        state.quote = None
    state.escaped = False
    return mask


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
    path: Path, line: str, state: SourceLiteralState, *, scan_line: list[str] | None = None
) -> list[bool]:
    suffix = path.suffix.casefold()
    if suffix in {".cs", ".lua", ".ps1"}:
        return _control_source_literal_mask(path, line, state, scan_line)
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
    path: Path, line: str, value_index: int, literal_mask: list[bool], value_length: int = 1
) -> bool:
    suffix = path.suffix.casefold()
    if suffix not in SOURCE_CODE_SUFFIXES:
        return True
    if suffix in MULTILINE_LITERAL_SUFFIXES:
        end = value_index + value_length
        return end <= len(literal_mask) and all(literal_mask[value_index:end])
    return _inside_quoted_literal(line, value_index)
