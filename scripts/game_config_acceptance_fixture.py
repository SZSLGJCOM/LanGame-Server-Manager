from pathlib import PurePosixPath, PureWindowsPath
import re
from typing import Any


WINDOWS_DEVICE_RE = re.compile(
    r"^(?:con|prn|aux|nul|con(?:in|out)\$|com[1-9]|lpt[1-9])(?:\..*)?$",
    re.IGNORECASE,
)
VALID_FILE_ROOTS = {"config", "install", "saves", "instance"}


def normalize_relative_path(value: Any) -> str | None:
    if not isinstance(value, str) or not value or "\\" in value or "\0" in value:
        return None
    posix = PurePosixPath(value)
    windows = PureWindowsPath(value)
    if (
        posix.is_absolute()
        or windows.is_absolute()
        or windows.drive
        or any(part in {"", ".", ".."} for part in value.split("/"))
    ):
        return None
    if any(
        any(ord(character) < 32 or character in '<>:"|?*' for character in part)
        or part.endswith((".", " "))
        or WINDOWS_DEVICE_RE.fullmatch(part) is not None
        for part in posix.parts
    ):
        return None
    normalized = posix.as_posix()
    return normalized if normalized not in {"", "."} else None


def validate_initial_files(
    prefix: str,
    fixture: dict[str, Any],
    failures: list[str],
) -> None:
    if "initial" not in fixture:
        return
    initial = fixture.get("initial")
    if not isinstance(initial, dict):
        failures.append(f"{prefix}: initial must be an object")
        return
    files = initial.get("files")
    if not isinstance(files, list) or not files:
        failures.append(f"{prefix}: initial.files must be a non-empty list")
        return

    rooted_paths: set[str] = set()
    for index, initial_file in enumerate(files):
        item_prefix = f"{prefix}: initial.files[{index}]"
        if not isinstance(initial_file, dict):
            failures.append(f"{item_prefix} must be an object")
            continue
        root = initial_file.get("root")
        if root not in VALID_FILE_ROOTS:
            failures.append(
                f"{item_prefix}.root must be one of {', '.join(sorted(VALID_FILE_ROOTS))}"
            )
        normalized = normalize_relative_path(initial_file.get("path"))
        if normalized is None:
            failures.append(f"{item_prefix} has unsafe relative path")
        else:
            initial_file["path"] = normalized
            rooted_path = (
                f"instance:config/{normalized}"
                if root == "config"
                else f"{root}:{normalized}"
            )
            if rooted_path in rooted_paths:
                failures.append(f"{item_prefix} duplicates initial path {rooted_path!r}")
            rooted_paths.add(rooted_path)
        content = initial_file.get("content")
        if not isinstance(content, (str, dict, list)):
            failures.append(f"{item_prefix}.content must be text, an object, or an array")


def validate_lifecycle(
    prefix: str,
    fixture: dict[str, Any],
    failures: list[str],
) -> None:
    if "lifecycle" not in fixture:
        return
    lifecycle = fixture.get("lifecycle")
    if not isinstance(lifecycle, dict):
        failures.append(f"{prefix}: lifecycle must be an object")
        return
    expected_keys = {"save_stage", "pre_start"}
    if set(lifecycle) != expected_keys:
        failures.append(f"{prefix}: lifecycle must contain exactly save_stage and pre_start")
        return
    for key in sorted(expected_keys):
        value = lifecycle.get(key)
        if not isinstance(value, str) or not value.strip():
            failures.append(f"{prefix}: lifecycle.{key} must be non-empty text")
