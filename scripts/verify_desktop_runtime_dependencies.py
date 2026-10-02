"""Reject desktop PE imports that require a separately installed Visual C++ runtime."""

import argparse
import hashlib
import json
import mmap
from pathlib import Path
import re
import struct

MAX_FILE_BYTES = 512 * 1024 * 1024
MAX_DESCRIPTORS = 4096
VC_RUNTIME = re.compile(r"^(?:vcruntime|msvcp|msvcr|concrt)\d[^/\\]*\.dll$", re.IGNORECASE)


class PeFormatError(ValueError):
    """A required PE structure cannot be read within its declared file bounds."""


def read_imports(data: bytes | mmap.mmap) -> dict[str, list[str]]:
    if not 64 <= len(data) <= MAX_FILE_BYTES:
        raise PeFormatError("PE file size is outside the supported bounds")

    def span(offset: int, size: int) -> None:
        if offset < 0 or size < 0 or offset + size > len(data):
            raise PeFormatError("PE structure extends beyond the file")

    def unpack(fmt: str, offset: int) -> tuple[int, ...]:
        span(offset, struct.calcsize(fmt))
        return struct.unpack_from(fmt, data, offset)

    pe = unpack("<I", 60)[0]
    if data[:2] != b"MZ" or pe < 64 or data[pe:pe + 4] != b"PE\0\0":
        raise PeFormatError("Missing DOS or PE signature")
    sections_count = unpack("<H", pe + 6)[0]
    optional_size = unpack("<H", pe + 20)[0]
    optional = pe + 24
    span(optional, optional_size)
    magic = unpack("<H", optional)[0]
    if magic == 0x10B:
        directory_start, image_base = 96, unpack("<I", optional + 28)[0]
    elif magic == 0x20B:
        directory_start, image_base = 112, unpack("<Q", optional + 24)[0]
    else:
        raise PeFormatError("Expected a PE32 or PE32+ optional header")
    if optional_size < directory_start:
        raise PeFormatError("Truncated optional header")
    directory_count = unpack("<I", optional + directory_start - 4)[0]
    if directory_count > 16 or directory_start + directory_count * 8 > optional_size:
        raise PeFormatError("Invalid data-directory table")
    section_table = optional + optional_size
    if not 1 <= sections_count <= 96:
        raise PeFormatError("Invalid section count")
    span(section_table, sections_count * 40)
    header_size = unpack("<I", optional + 60)[0]
    if not section_table + sections_count * 40 <= header_size <= len(data):
        raise PeFormatError("Invalid SizeOfHeaders")
    sections = []
    for index in range(sections_count):
        virtual_size, rva, raw_size, raw = unpack("<IIII", section_table + index * 40 + 8)
        span(raw, raw_size)
        if rva + max(virtual_size, raw_size) > 0x100000000:
            raise PeFormatError("Section RVA overflows the address space")
        sections.append((rva, virtual_size, raw, raw_size))

    def locate(rva: int, size: int) -> tuple[int, int]:
        matches = []
        if 0 <= rva < header_size and rva + size <= header_size:
            matches.append((rva, header_size - rva))
        for start, virtual_size, raw, raw_size in sections:
            delta = rva - start
            if 0 <= delta < max(virtual_size, raw_size):
                if delta + size > raw_size:
                    raise PeFormatError("RVA points to unbacked section data")
                matches.append((raw + delta, raw_size - delta))
        if len(matches) != 1:
            raise PeFormatError("Unmapped or ambiguous RVA")
        return matches[0]

    def dll_name(rva: int) -> str:
        if rva <= 0:
            raise PeFormatError("Invalid DLL name RVA")
        offset, available = locate(rva, 1)
        raw = data[offset:offset + min(available, 261)]
        end = raw.find(b"\0")
        if end < 1:
            raise PeFormatError("Missing or unterminated DLL name")
        raw = raw[:end]
        if any(byte < 32 or byte > 126 for byte in raw) or any(byte in raw for byte in b"/\\:"):
            raise PeFormatError("Invalid DLL import name")
        return raw.decode("ascii")

    def imports(directory: int, delay: bool) -> list[str]:
        if directory >= directory_count:
            return []
        rva, size = unpack("<II", optional + directory_start + directory * 8)
        if rva == size == 0:
            return []
        width = 32 if delay else 20
        if not rva or size < width:
            raise PeFormatError("Invalid import directory")
        offset, _ = locate(rva, size)
        names = []
        for index in range(min(size // width, MAX_DESCRIPTORS)):
            descriptor = unpack("<8I" if delay else "<5I", offset + index * width)
            if not any(descriptor):
                return sorted(set(names), key=lambda name: (name.casefold(), name))
            if delay:
                attributes, name = descriptor[:2]
                if attributes not in (0, 1):
                    raise PeFormatError("Unsupported delay-import attributes")
                if attributes == 0:
                    name -= image_base
            else:
                name = descriptor[3]
            names.append(dll_name(name))
        raise PeFormatError("Import directory has no bounded null terminator")

    return {"imports": imports(1, False), "delayImports": imports(13, True)}


def verify_binary(path: Path) -> dict[str, object]:
    with path.open("rb") as stream:
        size = stream.seek(0, 2)
        if not 64 <= size <= MAX_FILE_BYTES:
            raise PeFormatError("PE file size is outside the supported bounds")
        with mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
            imports = read_imports(data)
            sha = hashlib.sha256(data).hexdigest()
    forbidden = sorted({name for names in imports.values() for name in names if VC_RUNTIME.fullmatch(name)})
    return {"path": str(path), "sha256": sha, **imports, "forbiddenRuntimeImports": forbidden}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executables", nargs="+", type=Path)
    args = parser.parse_args()
    results = []
    for path in args.executables:
        try:
            results.append(verify_binary(path))
        except (OSError, PeFormatError) as error:
            results.append({"path": str(path), "error": str(error)})
    failed = any(item.get("error") or item.get("forbiddenRuntimeImports") for item in results)
    print(json.dumps({"status": "failed" if failed else "passed", "files": results}, ensure_ascii=False))
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
