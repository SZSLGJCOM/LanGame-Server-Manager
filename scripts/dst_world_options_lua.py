"""Restricted, non-executing reader for DST's first-party customization tables."""
from __future__ import annotations

from dataclasses import dataclass
import re


@dataclass(frozen=True)
class Reference:
    name: str


def remove_comments(source: str) -> str:
    result: list[str] = []
    index = 0
    while index < len(source):
        if source[index] in ('"', "'"):
            quote, start = source[index], index
            index += 1
            while index < len(source):
                if source[index] == "\\":
                    index += 2
                elif source[index] == quote:
                    index += 1
                    break
                else:
                    index += 1
            else:
                raise ValueError("Unsupported Lua: unterminated string")
            result.append(source[start:index])
        elif source.startswith("--", index):
            long_comment = re.match(r"--\[(=*)\[", source[index:])
            if long_comment:
                closing = "]" + long_comment.group(1) + "]"
                end = source.find(closing, index + len(long_comment.group(0)))
                if end < 0:
                    raise ValueError("Unsupported Lua: unterminated comment")
                index = end + len(closing)
            else:
                end = source.find("\n", index)
                index = end if end >= 0 else len(source)
            result.append(" ")
        else:
            result.append(source[index])
            index += 1
    return "".join(result)


def table_at(source: str, start: int) -> str:
    if source[start:start + 1] != "{":
        raise ValueError("Unsupported Lua: expected a literal table")
    quote: str | None = None
    escaped, depth = False, 0
    for index in range(start, len(source)):
        char = source[index]
        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
        elif char in ('"', "'"):
            quote = char
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[start:index + 1]
    raise ValueError("Unsupported Lua: unbalanced table")


def assignment_tables(source: str, name: str) -> list[str]:
    matches = re.finditer(r"\b" + re.escape(name) + r"\s*=\s*\{", source)
    return [table_at(source, match.end() - 1) for match in matches]


def assigned_table(source: str, name: str, *, local_only: bool = False) -> dict:
    tables = ([table_at(source, match.end() - 1) for match in re.finditer(r"\blocal\s+" + re.escape(name) + r"\s*=\s*\{", source)]
              if local_only else assignment_tables(source, name))
    if len(tables) != 1:
        raise ValueError(f"Missing or unsupported Lua table assignment: {name} ({len(tables)} found)")
    return parse_table(tables[0])


TOKEN = re.compile(
    r'''\s*("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*|-?\d+(?:\.\d+)?|\.\.|[{}\[\]=,;])'''
)


def _string(token: str) -> str:
    escapes = {"n": "\n", "r": "\r", "t": "\t", "b": "\b", "f": "\f", "\\": "\\", '"': '"', "'": "'"}

    def replace(match: re.Match) -> str:
        if match.group(1) not in escapes:
            raise ValueError(f"Unsupported Lua string escape: {match.group(0)}")
        return escapes[match.group(1)]

    return re.sub(r"\\(.)", replace, token[1:-1])


class _Parser:
    def __init__(self, source: str, text_expressions: bool) -> None:
        self.tokens: list[str] = []
        offset = 0
        while offset < len(source):
            match = TOKEN.match(source, offset)
            if not match:
                if source[offset:].strip():
                    raise ValueError(f"Unsupported Lua expression near {source[offset:offset + 55]!r}")
                break
            self.tokens.append(match.group(1))
            offset = match.end()
        self.index = 0
        self.text_expressions = text_expressions

    def peek(self, ahead: int = 0) -> str:
        return self.tokens[self.index + ahead] if self.index + ahead < len(self.tokens) else ""

    def take(self, expected: str | None = None) -> str:
        token = self.peek()
        if not token or (expected is not None and token != expected):
            raise ValueError(f"Unexpected Lua token {token!r}; expected {expected!r}")
        self.index += 1
        return token

    def value(self, depth: int = 0) -> object:
        if depth > 32:
            raise ValueError("Unsupported Lua: table nesting exceeds 32 levels")
        token = self.take()
        if token == "{":
            result: dict = {}
            next_index = 1
            while self.peek() != "}":
                if self.peek() == "[":
                    self.take("[")
                    key = self.value(depth + 1)
                    self.take("]")
                    self.take("=")
                elif self.peek(1) == "=":
                    key = self.take()
                    self.take("=")
                else:
                    key, next_index = next_index, next_index + 1
                if not isinstance(key, (str, int)) or isinstance(key, bool) or key in result:
                    raise ValueError(f"Unsupported or duplicate Lua table key: {key!r}")
                value = self.value(depth + 1)
                if key == "text" and self.text_expressions:
                    while self.peek() == "..":
                        self.take("..")
                        fragment = self.value(depth + 1)
                        if not isinstance(fragment, (Reference, str)):
                            raise ValueError("Unsupported Lua description text expression")
                result[key] = value
                if self.peek() in (",", ";"):
                    self.take()
                elif self.peek() != "}":
                    raise ValueError(f"Unexpected Lua token {self.peek()!r}; expected table separator")
            self.take("}")
            return result
        if token.startswith(('"', "'")):
            return _string(token)
        if token in ("true", "false", "nil"):
            return {"true": True, "false": False, "nil": None}[token]
        if re.fullmatch(r"-?\d+(?:\.\d+)?", token):
            return float(token) if "." in token else int(token)
        if re.fullmatch(r"[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*", token):
            return Reference(token)
        raise ValueError(f"Unexpected Lua value {token!r}")


def parse_table(source: str, *, text_expressions: bool = False) -> dict:
    parser = _Parser(remove_comments(source), text_expressions)
    value = parser.value()
    if parser.peek() or not isinstance(value, dict):
        raise ValueError("Unsupported Lua: expected one complete literal table")
    return value


def registrations(source: str, function: str, *, standalone: bool = False) -> dict[str, dict]:
    """Read literal registrations and a literal local table passed by name."""
    result: dict[str, dict] = {}
    spans: set[tuple[int, int]] = set()
    for match in re.finditer(r"\b" + function + r'''\("([^"\n]+)"\s*,\s*''', source):
        key = match.group(1)
        if key in result:
            raise ValueError(f"Duplicate Lua registration: {function}({key})")
        if source[match.end():match.end() + 1] == "{":
            text = table_at(source, match.end())
            end = match.end() + len(text)
            closing = re.match(r"\s*\)", source[end:])
            if not closing:
                raise ValueError(f"Unsupported Lua registration expression: {key}")
            result[key] = parse_table(text)
            spans.add((match.start(), end + closing.end()))
        else:
            reference = re.match(r"([A-Za-z_]\w*)\s*\)", source[match.end():])
            if not reference:
                raise ValueError(f"Unsupported Lua registration argument: {key}")
            result[key] = assigned_table(source, reference.group(1))
            spans.add((match.start(), match.end() + reference.end()))
            assignment = re.search(r"\blocal\s+" + reference.group(1) + r"\s*=\s*\{", source)
            if assignment is None:
                raise ValueError(f"Unsupported nonlocal registration table: {key}")
            spans.add((assignment.start(), assignment.end() - 1 + len(table_at(source, assignment.end() - 1))))
    if not result:
        raise ValueError(f"Missing literal Lua registrations: {function}")
    if standalone:
        residual = source
        for start, end in sorted(spans, reverse=True):
            residual = residual[:start] + residual[end:]
        if residual.strip():
            raise ValueError(f"Unsupported dynamic registration code in {function}: {residual.strip()[:60]!r}")
    return result
