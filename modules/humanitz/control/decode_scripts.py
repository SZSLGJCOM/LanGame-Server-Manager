"""Decode the bounded read-only native Script snapshot; never executes bytecode."""
from __future__ import annotations
import argparse
import hashlib
import json
import struct
from pathlib import Path


def parse(body: bytes, run: str, nonce: str, pid: int, creation_time: int) -> list[dict]:
    if not 0 < len(body) <= 32 * 1024 * 1024 + 16384:
        raise ValueError("snapshot exceeds its closed size bound")
    header, separator, payload = body.partition(b"\n\n")
    if not separator or len(header) > 256:
        raise ValueError("missing bounded header")
    fields = header.decode("ascii").split("\n")
    prefix = ["LGSM_HUMANITZ_SCRIPT_1", f"run={run}", f"nonce={nonce}", f"pid={pid}", f"creation_time={creation_time}"]
    if len(fields) != 6 or fields[:5] != prefix or not fields[5].startswith("count="):
        raise ValueError("snapshot identity does not match this owned process/request")
    text = fields[5][6:]
    if not text.isascii() or not text.isdecimal() or text.startswith("0") or not 1 <= int(text) <= 512:
        raise ValueError("invalid function count")
    result, seen, offset = [], set(), 0
    for _ in range(int(text)):
        if len(payload) - offset < 16:
            raise ValueError("truncated function record")
        address, flags, size = struct.unpack_from("<QII", payload, offset)
        offset += 16
        if not address or address in seen or size > 1024 * 1024 or size > len(payload) - offset or (flags & 0x400 and size):
            raise ValueError("invalid function body")
        seen.add(address)
        script = payload[offset:offset + size]
        offset += size
        result.append({"address": str(address), "flags": flags, "bytes": size,
                       "sha256": hashlib.sha256(script).hexdigest(), "hex": script.hex()})
    if offset != len(payload):
        raise ValueError("extra data after closed snapshot")
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("--run", required=True)
    parser.add_argument("--nonce", required=True)
    parser.add_argument("--pid", required=True, type=int)
    parser.add_argument("--creation-time", required=True, type=int)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    for text in [arguments.run, arguments.nonce]:
        if len(text) != 32 or any(value not in "0123456789abcdef" for value in text):
            raise ValueError("invalid owned run/nonce")
    if not 0 < arguments.pid <= 0xffffffff or not 0 < arguments.creation_time <= 0xffffffffffffffff:
        raise ValueError("invalid process identity")
    with arguments.input.open("rb") as source:
        body = source.read(32 * 1024 * 1024 + 16385)
    functions = parse(body, arguments.run, arguments.nonce, arguments.pid, arguments.creation_time)
    with arguments.output.open("x", encoding="utf-8") as destination:
        json.dump({"schema": 1, "module": "humanitz", "stage": "script_bytes_only", "run": arguments.run,
                   "nonce": arguments.nonce, "pid": arguments.pid, "creation_time": arguments.creation_time,
                   "functions": functions}, destination, indent=2)


if __name__ == "__main__":
    main()
