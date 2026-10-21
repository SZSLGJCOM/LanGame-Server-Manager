"""Inspect/install observers on an isolated actual native image, without game calls.

The caller supplies a disposable ordinary directory containing copies of the
reviewed GameAssembly.dll and baselib.dll. This does not initialize IL2CPP,
invoke a game method, create a world, or constitute real save acceptance.
"""
import ctypes
import hashlib
import json
import sys
from pathlib import Path

root = Path(sys.argv[1]).resolve(strict=True)
helper = Path(sys.argv[2]).resolve(strict=True)
expected = "ff366577fb0c707c42b8bde242304653aaa37f6ff0665ab9520150ceb692aed1"
image = root / "GameAssembly.dll"
with image.open("rb") as stream:
    if hashlib.file_digest(stream, "sha256").hexdigest() != expected:
        raise RuntimeError("unsupported_native_image")
kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.LoadLibraryExW.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p, ctypes.c_uint]
kernel.LoadLibraryExW.restype = ctypes.c_void_p
native = kernel.LoadLibraryExW(str(image), None, 0x100 | 0x800)
if not native:
    raise ctypes.WinError(ctypes.get_last_error())
bridge = ctypes.CDLL(str(helper))
bridge.lgsm_vr_save_install.restype = ctypes.c_int
bridge.lgsm_vr_save_diagnostic.restype = ctypes.c_int
result = bridge.lgsm_vr_save_install()
diagnostic = bridge.lgsm_vr_save_diagnostic()
print(json.dumps({"actual_image_sha256": expected, "native_hooks_installed": result == 1,
                  "diagnostic": diagnostic, "native_image_loaded": True,
                  "il2cpp_initialized": False, "game_method_invoked": False,
                  "save_acceptance": False}))
if result != 1:
    raise RuntimeError("actual_native_image_observer_install_failed")
if "--test-code-change" in sys.argv[3:]:
    kernel.VirtualProtect.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_uint, ctypes.POINTER(ctypes.c_uint)]
    kernel.VirtualProtect.restype = ctypes.c_int
    address = native + 0x7F339C0  # unhooked original MoveCopyAndFinalizeSave
    original = ctypes.string_at(address, 1)
    protection = ctypes.c_uint()
    if not kernel.VirtualProtect(address, 1, 0x40, ctypes.byref(protection)):
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        ctypes.memmove(address, bytes([original[0] ^ 1]), 1)
        if bridge.lgsm_vr_save_install() != 0:
            raise RuntimeError("modified_native_code_was_accepted")
    finally:
        ctypes.memmove(address, original, 1)
        prior = ctypes.c_uint()
        if not kernel.VirtualProtect(address, 1, protection.value, ctypes.byref(prior)):
            raise ctypes.WinError(ctypes.get_last_error())
    if bridge.lgsm_vr_save_install() != 1:
        raise RuntimeError("original_native_code_not_restored")
    print(json.dumps({"changed_native_code_rejected": True, "original_native_code_restored": True,
                      "game_method_invoked": False, "save_acceptance": False}))
