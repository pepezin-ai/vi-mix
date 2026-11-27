"""Call the GDK ABI in fresh processes so each DLL has its own queue and runtime state."""
import ctypes
import hashlib
import json
import os
import re
import subprocess
import sys
import threading
from pathlib import Path

import pytest

from test_runtime_cpp import CALLBACK, GAMERTAG_IID, USER_PROVIDER, GdkInterface, TaskQueueApi, add_local_user, guid_bytes
from test_runtime_gamer_picture import picture
from test_runtime_package import IID, PROVIDER
from conftest import RUNTIME_DIR

ROOT = Path(__file__).resolve().parents[1]
P = ctypes.c_void_p
U = ctypes.c_uint32
TOKEN = ctypes.c_uint64
IUNKNOWN = "00000000-0000-0000-C000-000000000046"
pytestmark = pytest.mark.skipif(sys.platform != "win32", reason="Windows DLL ABI")


def interface_matrix(folder):
    tables = (ROOT / "runtime/xgameruntime/served_interfaces.inc").read_text(encoding="utf-8")
    entries = re.findall(r'^ANYXBOXX_SERVED_INTERFACE\((\w+), "([^"]+)", "([^"]+)"\)', tables, re.M)
    gdk_table = (ROOT / "runtime/xgameruntime/gdk_api_table.inc").read_text(encoding="utf-8")
    slots = re.findall(r'^ANYXBOXX_GDK_API\((\w+), "([^"]+)", "([^"]+)", (\d+)\)', gdk_table, re.M)
    owners = {(p, i): name for name, p, i in entries}
    lengths = {}
    for _, provider, iid, slot in slots:
        name = owners.get((provider, iid))
        if name:
            lengths[name] = max(lengths.get(name, 0), int(slot) + 1)
    network = next((p, i) for name, p, i in entries if name == "NetworkingInterface")
    default_stub = GdkInterface(folder, *network).vtable[3]
    rows = []
    addresses = {}
    for name, provider, iid in entries:
        api = GdkInterface(folder, provider, iid)
        assert addresses.setdefault(name, api.object.value) == api.object.value
        query = api.method(0, U, P, ctypes.POINTER(P))
        row = []
        for candidate in [IUNKNOWN, *(entry[2] for entry in entries)]:
            out = P(1)
            code = query(guid_bytes(candidate), ctypes.byref(out))
            assert out.value in (None, api.object.value)
            row.append((code, bool(out.value)))
        row.extend([query(guid_bytes(iid), None), api.method(1, U)(), api.method(2, U)()])
        # Slot 8 of the task queue has its own reserved stub in VIctory.
        row.append([n for n in range(3, lengths[name]) if api.vtable[n] == default_stub
                    and (name, n) != ("TaskQueueInterface", 8)])
        rows.append(row)
    assert len(rows) == 26 and len(set(addresses.values())) == len(addresses)
    by_pair = {(p, i): row[-1] for (_, p, i), row in zip(entries, rows, strict=True)}
    refused = {api for api, p, i, slot in slots if int(slot) in by_pair.get((p, i), ())}
    # These handlers have their own ABI or diagnostics. They do not implement the service.
    refused.update(api for api, _, _, _ in slots if api.startswith("XUserGetTokenAndSignature"))
    refused.update({"XLaunchNewGame", "XLaunchUri", "XPersistentLocalStorageMountForPackage",
                    "XPackageUninstallChunks", "XPackageUninstallUWPInstance", "XPackageUninstallPackage"})
    inventory = (ROOT / "runtime/xgameruntime/unsupported.txt").read_text(encoding="utf-8").splitlines()
    assert sorted(refused) == inventory
    return rows


def exercise_wait(folder):
    api = TaskQueueApi(folder)
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateEventW.argtypes = [P, ctypes.c_int, ctypes.c_int, ctypes.c_wchar_p]
    kernel.CreateEventW.restype = P
    kernel.SetEvent.argtypes = kernel.CloseHandle.argtypes = [P]
    queue = P()
    assert api.method(12, U, U, U, ctypes.POINTER(P))(0, 0, ctypes.byref(queue)) == 0
    event = kernel.CreateEventW(None, False, False, None)
    assert event
    calls = []
    callback = CALLBACK(lambda context, canceled: calls.append((context, canceled)))
    register = api.method(20, U, P, U, P, P, CALLBACK, ctypes.POINTER(TOKEN))
    unregister = api.method(21, None, P, TOKEN)
    dispatch = api.method(16, ctypes.c_bool, P, U, U)
    token = TOKEN()
    try:
        invalid = [register(queue, 2, event, None, callback, ctypes.byref(token)),
                   register(queue, 1, None, None, callback, ctypes.byref(token)),
                   register(queue, 1, event, None, callback, None)]
        assert register(queue, 1, event, 37, callback, ctypes.byref(token)) == 0
        try:
            for _ in range(2):
                assert kernel.SetEvent(event)
                assert not dispatch(queue, 0, 0)
                assert dispatch(queue, 1, 5000)
        finally:
            unregister(queue, token)
        assert kernel.SetEvent(event)
        assert not dispatch(queue, 1, 100)
        # Terminating a composite must leave its owner's already queued wait intact.
        work, completion, composite = P(), P(), P()
        get_port = api.method(14, U, P, U, ctypes.POINTER(P))
        assert get_port(queue, 0, ctypes.byref(work)) == get_port(queue, 1, ctypes.byref(completion)) == 0
        assert api.method(13, U, P, P, ctypes.POINTER(P))(work, completion, ctypes.byref(composite)) == 0
        submitted = threading.Event()
        monitor_type = ctypes.WINFUNCTYPE(None, P, P, U)
        monitor = monitor_type(lambda *_: submitted.set())
        monitor_token = TOKEN()
        assert api.method(23, U, P, P, monitor_type, ctypes.POINTER(TOKEN))(
            queue, None, monitor, ctypes.byref(monitor_token)) == 0
        try:
            # The auto-reset event is still signaled from the unregister check.
            assert register(queue, 1, event, 39, callback, ctypes.byref(token)) == 0
            assert submitted.wait(5)
            submit = api.method(18, U, P, U, P, CALLBACK)
            assert submit(queue, 1, 41, callback) == 0
            assert api.method(22, U, P, ctypes.c_bool, P, P)(composite, False, None, None) == 0
            for _ in range(4):
                if len(calls) == 4:
                    break
                assert dispatch(queue, 1, 5000)
            unregister(queue, token)
        finally:
            api.method(24, None, P, TOKEN)(queue, monitor_token)
            api.method(17, None, P)(composite)
        return invalid, calls
    finally:
        api.method(17, None, P)(queue)
        kernel.CloseHandle(event)


def uninstall(folder):
    package = GdkInterface(folder, PROVIDER, IID)
    return [package.method(19, U, P, U, P)(None, 0, None),
            package.method(32, U, P)(None), package.method(34, ctypes.c_bool, P)(None)]


def entry_points(folder):
    dll = ctypes.WinDLL(str(folder / "xgameruntime.dll"))
    values = []
    for name in ("InitializeApiImpl", "InitializeApiImplEx", "InitializeApiImplEx2", "UninitializeApiImpl"):
        entry = getattr(dll, name)
        entry.argtypes, entry.restype = [P] * 8, U
        values.append(entry(*([None] * 8)))
    dll.XErrorReport.argtypes, dll.XErrorReport.restype = [U, ctypes.c_char_p], None
    dll.XErrorReport(0x80004001, b"synthetic ABI check")
    # IDXGIFactory is supported by all three DXGI entry points.
    for version in ("", "1", "2"):
        create = getattr(dll, "AnyXboxX_CreateDXGIFactory" + version)
        flags = [0] if version == "2" else []
        create.argtypes, create.restype = [U] * len(flags) + [P, ctypes.POINTER(P)], U
        for iid in ("7b7166ec-21c7-44ae-b21a-c9ae321ae369", "00000000-0000-0000-0000-000000000001"):
            out = P()
            code = create(*flags, guid_bytes(iid), ctypes.byref(out))
            values.append((code, bool(out)))
            if out:
                vtable = ctypes.cast(out, ctypes.POINTER(ctypes.POINTER(P)))[0]
                ctypes.WINFUNCTYPE(U, P)(vtable[2])(out)
    return values


def probe(folder):
    tasks = TaskQueueApi(folder)
    tasks.method(27, U, ctypes.c_bool)(True)
    verify = tasks.method(28, U)
    thread_status = [verify()]
    worker = threading.Thread(target=lambda: thread_status.append(verify()))
    worker.start()
    worker.join()
    tasks.method(29, None)()
    tasks.method(27, U, ctypes.c_bool)(False)
    refusals = uninstall(folder)
    users, _, _, user = add_local_user(folder)
    tag = GdkInterface(folder, USER_PROVIDER, GAMERTAG_IID)
    buffer = ctypes.create_string_buffer(16)
    assert tag.method(3, U, P, U, ctypes.c_size_t, P, P)(user, 0, 16, buffer, None) == 0
    users.method(4, None, P)(user)
    pictures = [hashlib.sha256(picture(folder, size)[1]).hexdigest() for size in range(4)]
    return dict(threads=thread_status, exports=entry_points(folder), gamertag=buffer.value.hex(), interfaces=interface_matrix(folder), wait=exercise_wait(folder), images=pictures,
                uninstall=refusals)


def test_observable_results_match_cpp(runtime_dir):
    oracle = RUNTIME_DIR.parent / "oracle"
    if not (oracle / "xgameruntime.dll").is_file():
        if os.environ.get("ANYXBOXX_REQUIRE_RUNTIME") == "1":
            pytest.fail("build the xgameruntime_cpp oracle with CMake")
        pytest.skip("C++ oracle not built")
    results = []
    for folder in (oracle, runtime_dir):
        proc = subprocess.run([sys.executable, __file__, str(folder)], capture_output=True,
                              env={**os.environ, "ANYXBOXX_TRACE": "1", "ANYXBOXX_SYSTEM_RUNTIME": "0",
                                   "ANYXBOXX_GAMERTAG": "René"},
                              timeout=60)
        assert proc.returncode == 0, proc.stderr[-4000:]
        assert b"[XErrorReport] 0x80004001 synthetic ABI check" in proc.stderr
        assert b"thread sensible au temps" in proc.stderr
        results.append(json.loads(proc.stdout))
    cpp, rust = results
    assert cpp.pop("uninstall") == [0, 0x80004001, False]
    assert rust.pop("uninstall") == [0x80004001, 0x80004001, False]
    assert rust == cpp
    assert rust["threads"] == [0x80004005, 0]
    assert rust["wait"] == [[0x80070057, 0x80070057, 0x80004003], [[37, False], [37, False], [39, False], [41, False]]]


def refusal_probe(folder):
    def interface(provider, iid=None):
        return GdkInterface(folder, provider, iid or provider)

    user = interface(USER_PROVIDER)
    display = interface("1B339674-328D-4283-A200-3171F18D3639")
    storage = interface("F4FAF4D4-2D04-4FCE-B3E0-474A713A3E84", "41A4E10C-5A7E-41D9-8C37-37BDE62A07D6")
    pending = interface("0651AAE2-4012-4077-BF84-8B9097090E2C", "014D1CC3-BCFE-41FF-B2F0-E1EF07155828")
    return uninstall(folder) + [user.method(24, U, P, P)(None, None),
                                display.method(3, U, P, P)(None, None),
                                storage.method(8, U, P, P)(None, None),
                                pending.method(7, U, P)(None), TaskQueueApi(folder).method(8, U)()]


def test_refusals_are_logged_with_tracing_disabled(runtime_dir):
    result = subprocess.run([sys.executable, __file__, str(runtime_dir), "refusals"],
                            capture_output=True, timeout=20, check=True,
                            env={**os.environ, "ANYXBOXX_TRACE": "0", "ANYXBOXX_SYSTEM_RUNTIME": "0"})
    assert json.loads(result.stdout) == [0x80004001, 0x80004001, False] + [0x80004001] * 5
    for api in (b"XPackageUninstallChunks", b"XPackageUninstallUWPInstance", b"XPackageUninstallPackage",
                b"XUser Xbox Live token", b"XLaunchUri", b"XPersistentLocalStorageMountForPackage",
                b"E_NOTIMPL", b"XTaskQueue slot 8"):
        assert api in result.stderr


if __name__ == "__main__":
    action = refusal_probe if sys.argv[2:] == ["refusals"] else probe
    print(json.dumps(action(Path(sys.argv[1]))))
