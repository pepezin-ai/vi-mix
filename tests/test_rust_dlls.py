"""Check the installed Rust runtime against an independently built C++ oracle."""
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import pefile
import pytest

from conftest import RUNTIME_DIR

REPO = Path(__file__).resolve().parents[1]
DLL = "xgameruntime.dll"
RUST = REPO / os.environ.get("CARGO_TARGET_DIR", "target") / "release" / DLL
ORACLE = RUNTIME_DIR.parent / "oracle" / DLL
SUITE = [str(p.relative_to(REPO)) for p in sorted((REPO / "tests").glob("test_runtime_*.py"))]
SUITE += ["tests/test_gdk_boot.py", "tests/test_native_services.py", "tests/test_saves.py",
          "tests/test_perf.py", "tests/test_package.py"]
pytestmark = pytest.mark.skipif(sys.platform != "win32", reason="Windows DLLs")
# Skip reasons that only mean the machine lacks an optional component.
ALLOWED_SKIPS = ("runtime GDK de Windows absent",)


@pytest.fixture
def built(runtime_dir):
    for dll in (RUST, ORACLE):
        if not dll.is_file():
            if os.environ.get("ANYXBOXX_REQUIRE_RUNTIME") == "1":
                pytest.fail(f"Missing runtime artifact {dll}")
            pytest.skip(f"Missing runtime artifact {dll}")
    return runtime_dir / DLL


@pytest.mark.parametrize("cpp", [False, True], ids=["rust", "cpp"])
def test_runtime_suite(tmp_path, built, cpp):
    folder = tmp_path / "bin"
    shutil.copytree(built.parent, folder)
    if cpp:
        shutil.copyfile(ORACLE, folder / DLL)
    env = {**os.environ, "ANYXBOXX_RUNTIME": str(folder), "ANYXBOXX_REQUIRE_RUNTIME": "1",
           "ANYXBOXX_TEST_CPP_ORACLE": str(int(cpp)), "ANYXBOXX_SYSTEM_RUNTIME": "0"}
    done = subprocess.run([sys.executable, "-m", "pytest", "-q", "-rs", "-p", "no:cacheprovider",
                           f"--basetemp={tmp_path / 'nested'}", *SUITE],
                          cwd=REPO, env=env, capture_output=True, text=True, timeout=600)
    summary = done.stdout.strip().splitlines()[-1] if done.stdout.strip() else done.stderr
    assert done.returncode == 0, done.stdout[-4000:] + done.stderr[-2000:]
    passed = re.search(r"(\d+) passed", summary)
    skips = [line for line in done.stdout.splitlines() if line.startswith("SKIPPED")]
    # Only skips that depend on the machine are accepted: the Windows GDK runtime is not
    # installed on CI, and that test compares against it, not against our DLL.
    unexpected = [line for line in skips if not any(reason in line for reason in ALLOWED_SKIPS)]
    assert passed and int(passed.group(1)) > 0 and not unexpected, (summary, unexpected)


def exports(path):
    with pefile.PE(str(path), fast_load=True) as image:
        image.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_EXPORT"]])
        return {(s.ordinal, s.name, s.forwarder) for s in image.DIRECTORY_ENTRY_EXPORT.symbols}


def test_installed_dll_is_rust_and_matches_cpp_exports(built):
    assert built.read_bytes() == RUST.read_bytes()
    assert built.read_bytes() != ORACLE.read_bytes()
    assert exports(built) == exports(ORACLE)
