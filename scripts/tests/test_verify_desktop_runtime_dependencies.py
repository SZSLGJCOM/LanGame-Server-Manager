import contextlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

from scripts.verify_desktop_runtime_dependencies import PeFormatError, main, read_imports, verify_binary


def pe_fixture(import_name=b"KERNEL32.dll", delay_name=b"msvcrt.dll", *, pe32=False, delay_va=False):
    data = bytearray(1536)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 128)
    data[128:132] = b"PE\0\0"
    optional, directory_start = 152, 96 if pe32 else 112
    optional_size = directory_start + 128
    struct.pack_into("<HH", data, 132, 0x14C if pe32 else 0x8664, 1)
    struct.pack_into("<H", data, 148, optional_size)
    struct.pack_into("<H", data, optional, 0x10B if pe32 else 0x20B)
    struct.pack_into("<I" if pe32 else "<Q", data, optional + (28 if pe32 else 24),
                     0x400000 if pe32 else 0x140000000)
    struct.pack_into("<I", data, optional + 60, 512)
    struct.pack_into("<I", data, optional + directory_start - 4, 16)
    struct.pack_into("<II", data, optional + directory_start + 8, 0x1000, 40)
    struct.pack_into("<II", data, optional + directory_start + 13 * 8, 0x1040, 64)
    section = optional + optional_size
    data[section:section + 8] = b".rdata\0\0"
    struct.pack_into("<IIII", data, section + 8, 1024, 0x1000, 1024, 512)
    struct.pack_into("<5I", data, 512, 0, 0, 0, 0x1100, 0)
    struct.pack_into("<8I", data, 576, 0 if delay_va else 1,
                     0x401180 if delay_va else 0x1180, 0, 0, 0, 0, 0, 0)
    data[768:768 + len(import_name) + 1] = import_name + b"\0"
    data[896:896 + len(delay_name) + 1] = delay_name + b"\0"
    return data


class DesktopRuntimeDependencyTests(unittest.TestCase):
    def test_reads_real_import_structures_and_legacy_delay_va(self):
        for pe32, delay_va in ((False, False), (True, False), (True, True)):
            with self.subTest(pe32=pe32, delay_va=delay_va):
                self.assertEqual(read_imports(pe_fixture(pe32=pe32, delay_va=delay_va)),
                                 {"imports": ["KERNEL32.dll"], "delayImports": ["msvcrt.dll"]})

    def test_rejects_separate_vc_runtime_in_both_import_tables(self):
        names = (b"VCRUNTIME140.dll", b"vcruntime140_1.dll", b"MSVCP140.dll", b"msvcr120.dll", b"concrt140.dll")
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "desktop.exe"
            for name in names:
                for delayed in (False, True):
                    with self.subTest(name=name, delayed=delayed):
                        path.write_bytes(pe_fixture(delay_name=name) if delayed else pe_fixture(import_name=name))
                        self.assertEqual(verify_binary(path)["forbiddenRuntimeImports"], [name.decode()])

    def test_allows_windows_system_crt_and_api_sets(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "desktop.exe"
            path.write_bytes(pe_fixture(b"api-ms-win-crt-runtime-l1-1-0.dll", b"msvcp_win.dll"))
            self.assertEqual(verify_binary(path)["forbiddenRuntimeImports"], [])
            path.write_bytes(pe_fixture(delay_name=b"MSVCRT.DLL"))
            self.assertEqual(verify_binary(path)["forbiddenRuntimeImports"], [])

    def test_rejects_malformed_headers_and_unmapped_directory(self):
        cases = []
        data = pe_fixture(); data[0:2] = b"NO"; cases.append(data)
        data = pe_fixture(); struct.pack_into("<I", data, 60, 0xFFFFFF00); cases.append(data)
        data = pe_fixture(); struct.pack_into("<H", data, 134, 97); cases.append(data)
        data = pe_fixture(); struct.pack_into("<H", data, 148, 10); cases.append(data)
        data = pe_fixture(); struct.pack_into("<I", data, 260, 17); cases.append(data)
        data = pe_fixture(); struct.pack_into("<I", data, 272, 0x9999); cases.append(data)
        data = pe_fixture(); struct.pack_into("<I", data, 408, 4096); cases.append(data)
        data = pe_fixture(); struct.pack_into("<H", data, 134, 2)
        data[432:472] = data[392:432]; cases.append(data)
        for index, data in enumerate(cases):
            with self.subTest(index=index), self.assertRaises(PeFormatError):
                read_imports(data)
        with self.assertRaises(PeFormatError):
            read_imports(b"MZ")

    def test_rejects_unbacked_rva_and_unterminated_imports_or_names(self):
        cases = []
        data = pe_fixture(); struct.pack_into("<I", data, 400, 2048)
        struct.pack_into("<I", data, 524, 0x1500); cases.append(data)
        data = pe_fixture(); struct.pack_into("<I", data, 276, 20); cases.append(data)
        data = pe_fixture(); data[768:1029] = b"A" * 261; cases.append(data)
        data = pe_fixture(); struct.pack_into("<I", data, 576, 2); cases.append(data)
        data = pe_fixture(); struct.pack_into("<II", data, 524, 0, 0x1200); cases.append(data)
        data = pe_fixture(); struct.pack_into("<I", data, 584, 1)
        struct.pack_into("<I", data, 612, 0x1180); cases.append(data)
        data = pe_fixture(import_name=b"../VCRUNTIME140.dll"); cases.append(data)
        for index, data in enumerate(cases):
            with self.subTest(index=index), self.assertRaises(PeFormatError):
                read_imports(data)

    def test_cli_checks_all_requested_files_and_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            good, bad = Path(directory) / "desktop.exe", Path(directory) / "install_catalog.exe"
            good.write_bytes(pe_fixture()); bad.write_bytes(pe_fixture(delay_name=b"VCRUNTIME140.dll"))
            output = io.StringIO()
            with patch("sys.argv", ["verify", str(good), str(bad)]), contextlib.redirect_stdout(output):
                self.assertEqual(main(), 1)
            report = json.loads(output.getvalue())
            self.assertEqual(report["status"], "failed")
            self.assertEqual(len(report["files"]), 2)
            self.assertEqual(report["files"][1]["forbiddenRuntimeImports"], ["VCRUNTIME140.dll"])
            with patch("sys.argv", ["verify", str(good)]), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(main(), 0)
            with patch("sys.argv", ["verify", str(bad.with_suffix('.missing'))]), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(main(), 1)


if __name__ == "__main__":
    unittest.main()
