"""Regenerate the reviewed native function ranges from the exact server image."""
import argparse
import hashlib
import struct
from pathlib import Path

IMAGE_SHA256 = "4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad"
# Full AutoSave stages, native publication/drain/cancellation, connection,
# transaction wrappers, and actual SQLite 3.39.4 VM/Windows VFS span.
ROOTS = (
    0x1b94040,0x1ba7060,0x1b94ee0,0x1b94250,0x1b98390,0x1ba60e0,
    0x1ba6380,0x1b979f0,0x1b976c0,0x1b5be10,0x1b90440,0x1bb27c0,
    0x1abced0,0x1ad3aa0,0x1acea90,0x1ab9df0,0x1ab5d90,0x1ac82d0,
    0x1a4c3e0,0x1ac3090,0x1ac6710,0x1acc820,0x1accee0,0x1ac4d30,
    0x1ab4ad0,0x1ab6640,0x1ad39c0,0x1ae3b70,0x1ae9390,0x1afe8d0,
    0x271d4a0,0x1bd6a70,0x2cfc330,
    # Exact item producers, every item writer, typed publication adapters, and
    # their real storage clone/get/destruction contracts. No wildcard callback.
    0x1ba6690,0x1bb2bc0,0x1b5bca0,0x1b998a0,0x1bc8d30,0x1bc99e0,
    0x1bcab30,0x1bcadd0,0x1bcbd30,0x1bcc320,0x1bcc4c0,0x1bcc850,
    0x1bcedf0,0x1bd07d0,0x1b97290,0x1b97340,0x1b973b0,0x1b97650,
    0x1b97750,0x1b978f0,0x1b97b10,0x1b91ec0,0x1b981a0,0x1b98290,0x1b98300,
    0x1b98080,0x1b92150,0x1b98210,0x1ba67e0,0x1b8ca60,0x1ba4f20,
    0x1ae0cd0,0x1ae0d00,0x1ae0dc0,0x1ae0de0,0x1ae0e00,0x1ae0e20,
    0x929d00,0xd3d910,0xf54870,0x16495a0,0x16d0f70,0x195e390,
    0x1b98b00,0x1b98c30,0x955310,0x9f1f60,0x9f1f10,0x163fda0,
    0x90dcd0,0x90dd00,0x9f2080,0x1b925b0,
    # Accepted descriptor factories, typed cache create/lifecycle, and the
    # exact nested Function64 destructor; generic UE/CRT/allocator calls remain boundaries.
    0x1bb2cc0,0x1bb26c0,0x9eee60,0x1abebb0,0x1ba4ca0,0x1ba4d20,0x1ba4fa0,
    0x1b9ef40,0x1b9f2d0,0x1ba0380,0x1b8e520,0x1ba09c0,0x1b8e6a0,0x1aae550,0x1adb4f0,
    # Three exact AItemVirtualizationManager producers, nested callable lifecycle,
    # Actor world predicate and SQL wrappers. Generic UE/CRT/allocator calls are boundaries.
    0x1e2c5b0,0x1959490,0x1e25ad0,0x1e32980,0x1e45010,0x1e29530,0x1e20080,0x1e243a0,0xcdee60,0xcd3d20,0x1e28cb0,0x1e23eb0,0x1e234b0,0x3ecd490,0x1b01510,0x1adb670,0x1adb690,0x1adbcf0,0x1aff2c0,0x1adc110,0x1adc150,0x1adbf60,0x1ae0da0,0x1ae0e50,
)
TASK_LEAVES=((0x9108c0,5),(0x964310,5),(0x90efd0,0x22),(0x90f5b0,9),
             (0x9f4040,0x22),(0x1ac74c0,5),(0x1b97ff0,8),
             (0x1e28e30,0x8),(0x1e28db0,0x8),(0x2d06c00,0x2e),(0x1ba71b0,0x1c),(0x1adbce0,0x9),(0x1ae3b60,0xf))
TASK_TABLES=((0x571e488,4),(0x59795d8,4),(0x59df9b8,4),(0x58c4b08,4),
             (0x5196950,4),(0x51e8bd8,4),(0x58c9660,4),(0x59df8d8,4),(0x59d8488,3),
             (0x59dde98,3),(0x59db878,3),(0x59db318,3),
             (0x5a73b78,4),(0x56c6b10,4),(0x5a68c60,1))
FOLDED_SQLITE_LEAVES=(0x9ad010,0xb6c9b0,0x90aa40,0xa4ca90,0x16ddd00,0x90aba0,
                      0x9f7f60,0xec0ff0,0x12d7470,0x90abb0,0x133ace0,0xadfa80)



def generate(image: Path) -> str:
    data = image.read_bytes()
    if hashlib.sha256(data).hexdigest() != IMAGE_SHA256:
        raise ValueError("The native server image does not match the reviewed build")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    count, optional = struct.unpack_from("<H", data, pe + 6)[0], pe + 24
    section_table = optional + struct.unpack_from("<H", data, pe + 20)[0]
    sections = []
    for index in range(count):
        at = section_table + index * 40
        virtual, size, raw = struct.unpack_from("<III", data, at + 12)
        sections.append((virtual, size, raw))

    def offset(rva: int, length: int = 1) -> int:
        for virtual, size, raw in sections:
            if virtual <= rva and rva + length <= virtual + size:
                return raw + rva - virtual
        raise ValueError(f"Unmapped file RVA {rva:x}")

    rva, size = struct.unpack_from("<II", data, optional + 112 + 3 * 8)
    entries = list(struct.iter_unpack("<III", data[offset(rva):offset(rva) + size]))

    def owner(entry):
        seen = set()
        while True:
            unwind = entry[2]
            if unwind in seen:
                raise ValueError("Cyclic native unwind metadata")
            seen.add(unwind)
            at = offset(unwind)
            if not (data[at] >> 3) & 4:
                return entry
            codes = data[at + 2]
            entry = struct.unpack_from("<III", data, at + 4 + ((codes + 1) & ~1) * 2)

    groups = {}
    for entry in entries:
        groups.setdefault(owner(entry), []).append(entry)
    selected={(0x15501c0,0x15501c0+21,0),(0x2d230b0,0x2d2310a,0)}
    selected.update((leaf,leaf+16,0) for leaf in FOLDED_SQLITE_LEAVES)
    selected.update((leaf,leaf+size,0) for leaf,size in TASK_LEAVES)
    # Exact virtualization SQL literals are part of the accepted writer contract.
    selected.update((rva,rva+size,0) for rva,size in ((94804936, 58), (94805008, 108), (94805120, 192), (94805312, 188), (94803520, 212), (94805624, 56)))
    for begin in range(0x28dc000,0x29a9000,0xc000):
        selected.add((begin,min(begin+0xc000,0x29a9000),0))
    for root in ROOTS:
        matches = [entry for entry in entries if entry[0] <= root < entry[1]]
        if len(matches) != 1:
            raise ValueError(f"Missing/ambiguous native function {root:x}")
        selected.update(groups[owner(matches[0])])
    lines = [
        "// Generated by generate_profile.py from the checksum-pinned server.",
        "// Includes every chained unwind fragment of each reviewed logical root.",
        "#pragma once",
        "#include <cstdint>",
        "namespace scum_save {",
        "struct NativeRange { std::uint32_t rva, bytes; };",
        "inline constexpr NativeRange NATIVE_RANGES[] = {",
    ]
    for begin, end, _ in sorted(selected):
        offset(begin, end - begin)
        lines.append(f"    {{0x{begin:x}, 0x{end - begin:x}}},")
    lines += ["};", "struct ImportSlot { const char* name; std::uint32_t rva; };",
              "inline constexpr ImportSlot IMPORT_SLOTS[] = {"]
    requested = {"CreateFileW", "ReadFile", "WriteFile", "GetOverlappedResult", "FlushFileBuffers", "CloseHandle", "DeleteFileW", "SetFileAttributesW", "CreateFileA", "DeleteFileA", "SetEndOfFile", "CreateFileMappingW", "EnterCriticalSection"}
    found = set()

    def string(rva):
        start = offset(rva)
        end = data.index(0, start, start + 256)
        return data[start:end].decode("ascii")

    imports, _ = struct.unpack_from("<II", data, optional + 112 + 8)
    descriptor = offset(imports)
    while True:
        lookup, _, _, name, address = struct.unpack_from("<IIIII", data, descriptor)
        if not name:
            break
        library = string(name).lower()
        index = 0
        while True:
            target = struct.unpack_from("<Q", data, offset(lookup + index * 8, 8))[0]
            if not target:
                break
            if not target >> 63:
                symbol = string(target + 2)
                if symbol in requested:
                    if library != "kernel32.dll" or symbol in found:
                        raise ValueError("Unexpected native IO import ownership")
                    found.add(symbol)
                    lines.append(f'    {{"{symbol}", 0x{address + index * 8:x}}},')
            index += 1
        descriptor += 20
    if found != requested:
        raise ValueError("Missing native IO imports")
    lines += ["};", "struct NativeSlot { std::uint32_t rva,target; };", "inline constexpr NativeSlot NATIVE_SLOTS[] = {"]
    base=struct.unpack_from("<Q",data,optional+24)[0]
    for index in range(265):
        slot=0x6162700+index*8
        target=struct.unpack_from("<Q",data,offset(slot,8))[0]
        target=target-base if target else 0
        if target and not (0x28dc000<=target<0x29a9000 or target in FOLDED_SQLITE_LEAVES):
            raise ValueError(f"SQLite API target is outside reviewed code: {slot:x}")
        lines.append(f"    {{0x{slot:x},0x{target:x}}},")
    lines.append("    {0x5e40c60+0x160,0x15501c0},")
    for table,count in TASK_TABLES:
        for index in range(count):
            slot=table+index*8;target=struct.unpack_from("<Q",data,offset(slot,8))[0]-base
            if not any(begin<=target<end for begin,end,_ in selected):
                raise ValueError(f"Unreviewed task storage target: {slot:x}->{target:x}")
            lines.append(f"    {{0x{slot:x},0x{target:x}}},")
    relocation_rva,relocation_size=struct.unpack_from("<II",data,optional+112+5*8)
    current=offset(relocation_rva);end=current+relocation_size
    while current<end:
        page,block=struct.unpack_from("<II",data,current)
        if block<8 or current+block>end:raise ValueError("Invalid relocation metadata")
        for at in range(current+8,current+block,2):
            item=struct.unpack_from("<H",data,at)[0]
            if item>>12:
                address=page+(item&4095)
                if any(begin<=address<end for begin,end,_ in selected):
                    raise ValueError(f"Selected code contains relocated bytes: {address:x}")
        current+=block
    return "\n".join(lines+["};","}",""])



if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    args = parser.parse_args()
    output = Path(__file__).with_name("native_profile_data.hpp")
    output.write_text(generate(args.image), encoding="utf-8", newline="\n")
    print(f"Generated {output.name}")
