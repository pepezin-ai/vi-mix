"""Parity of `anyxboxx carte` (crates/anyxboxx-report/src/status_map) with `scarlett.py --carte`.

Both commands map the same package, runtime and TomBox folders built from source by pe_builder: the
SVG files must be byte-identical and the badges printed the same, or both refuse. A second set of
cases feeds both sides synthetic GDK tables (runtime/xgameruntime of a temporary repository for the
Rust side, the same files passed to emu/status_map.gdk_blocks for the Python side).
"""
import os
import subprocess
import sys

import pytest

from emu import status_map, translation
from pe_builder import Asm, build_pe
from test_rust_traduction_parity import (
    REPO, SCARLETT, environment, gpu_coverage_table, make, require_binary, write_registry)

CODE = Asm().mov_eax(1).ret().build()


@pytest.fixture(scope="module")
def binary():
    return require_binary()


@pytest.fixture(scope="module")
def registry(tmp_path_factory):
    return write_registry(tmp_path_factory.mktemp("registry") / "emulated.txt")


@pytest.fixture(scope="module")
def gpu_table(tmp_path_factory):
    path = tmp_path_factory.mktemp("gpu") / "rdna2-coverage.txt"
    path.write_text(gpu_coverage_table(), encoding="utf-8")
    return path


def run(command, folders, svg):
    done = subprocess.run(command, capture_output=True, text=True, encoding="utf-8", timeout=300,
                          env=environment(folders))
    image = svg.read_bytes() if svg.is_file() else None
    return done.returncode == 0, done.stdout, image, done.stderr


def python_map(folders, svg):
    return run([sys.executable, str(SCARLETT), "--carte", str(folders["package"]), str(svg)], folders, svg)


def rust_map(binary, folders, svg, *options):
    return run([str(binary), "carte", str(folders["package"]), str(svg), *map(str, options)], folders, svg)


def many_modules(tmp):
    """A dozen modules of very different sizes: a treemap with rows and columns, tiny and large cells."""
    folders = make(tmp, tombox=True)
    imports = [("xmem.dll", f"XMem{n}") for n in range(3)] + [("kernel32.dll", "Sleep")]
    for index in range(12):
        build_pe(folders["package"] / f"module{index:02}_with_a_long_name.dll", lambda iat: CODE,
                 imports=imports[: 1 + index % 4] + [("absent.dll", f"F{n}") for n in range(index * 9)])
    build_pe(folders["package"] / "x&y.dll", lambda iat: CODE, imports=[("absent.dll", "A")])
    return folders


CASES = [
    ("small-package", lambda tmp: make(tmp)),
    ("with-tombox", lambda tmp: make(tmp, tombox=True)),
    ("no-runtime-folder", lambda tmp: make(tmp, runtime=False)),
    ("no-imports", lambda tmp: make(tmp, game=[], helper=False)),
    ("many-modules", many_modules),
]


@pytest.mark.parametrize("make_case", [c[1] for c in CASES], ids=[c[0] for c in CASES])
def test_rust_map_and_badges_equal_python(binary, registry, gpu_table, tmp_path, make_case):
    folders = make_case(tmp_path)
    python = python_map(folders, tmp_path / "python" / "carte.svg")
    rust = rust_map(binary, folders, tmp_path / "rust" / "carte.svg", "--emulated-list", registry,
                    "--gpu-coverage", gpu_table)
    assert python[0], python[3]
    assert rust[:3] == python[:3], rust[3]
    assert rust[1].startswith("CPU x86-64 : 100%\nGDK runtime : ")


def test_both_refuse_an_image_below_a_file(binary, registry, gpu_table, tmp_path):
    folders = make(tmp_path)
    (tmp_path / "file").write_text("x", encoding="utf-8")
    svg = tmp_path / "file" / "carte.svg"
    python = python_map(folders, svg)
    rust = rust_map(binary, folders, svg, "--emulated-list", registry, "--gpu-coverage", gpu_table)
    assert not python[0] and not rust[0]
    assert python[1] == rust[1] == ""
    assert rust[3].startswith("anyxboxx carte: ")


API = 'ANYXBOXX_GDK_API({name}, "{provider}", "{interface}", {method})'
SERVED = 'ANYXBOXX_SERVED_INTERFACE(Object, "{provider}", "{interface}")'


def gdk_sources(folder, apis, served, stubs):
    """runtime/xgameruntime of a temporary repository: the three kinds of files the map reads."""
    folder.mkdir(parents=True)
    lines = ["// ANYXBOXX_GDK_API(fonction, fournisseur, interface, n° de méthode)"]
    lines += [API.format(name=n, provider=p, interface=i, method=m) for m, (n, p, i) in enumerate(apis)]
    (folder / "gdk_api_table.inc").write_text("\n".join(lines) + "\n", encoding="utf-8")
    (folder / "served_interfaces.inc").write_text(
        "\n".join(SERVED.format(provider=p.lower(), interface=i) for p, i in served) + "\n", encoding="utf-8")
    (folder / "a.cpp").write_text("".join(f"    ANYXBOXX_NOT_IMPLEMENTED,\n    // 4 {s}\n" for s in stubs),
                                  encoding="utf-8")
    (folder / "b.CPP").write_text("ANYXBOXX_NOT_IMPLEMENTED, // 9 XUserGetId\n", encoding="utf-8")
    (folder / "c.h").write_text("ANYXBOXX_NOT_IMPLEMENTED, // 9 XTaskQueueClose\n", encoding="utf-8")


GDK_CASES = {
    "groups-ties-and-stubs": (
        [("XUserAddAsync", "P1", "I1"), ("XUserGetId", "P1", "I1"), ("XGameUiShow", "P1", "I1"),
         ("XTaskQueueCreate", "P2", "I2"), ("XTaskQueue_Close", "P2", "I2"), ("XTaskQueueClose", "P2", "I2"),
         ("XBetaOne", "P3", "I3"), ("XAlphaTwo", "P3", "I3"), ("XStoreQuery", "P4", "I4")],
        [("P1", "I1"), ("P2", "I2"), ("P4", "I4"), ("P9", "I9")], ["XStoreQuery", "XUserAddAsync"]),
    "same-name-for-two-interfaces": (
        [("XUserA", "P1", "I1"), ("XUserB", "P1", "I1"), ("XUserC", "P2", "I2")] +
        [(f"XPackageFunction{n:03}", "P3", "I3") for n in range(150)],
        [("P2", "I2"), ("P3", "I3")], [f"XPackageFunction{n:03}" for n in range(0, 150, 7)]),
    "nothing-served": ([("XAppOne", "P1", "I1")], [], []),
}


@pytest.mark.parametrize("case", list(GDK_CASES), ids=list(GDK_CASES))
def test_rust_reads_the_gdk_tables_like_python(binary, registry, gpu_table, tmp_path, monkeypatch, case):
    folders = make(tmp_path)
    repo = tmp_path / "repo"
    sources = repo / "runtime" / "xgameruntime"
    gdk_sources(sources, *GDK_CASES[case])
    original = status_map.gdk_blocks
    monkeypatch.setattr(status_map, "gdk_blocks", lambda: original(
        sources / "gdk_api_table.inc", sources / "served_interfaces.inc", sources))
    python = status_map.render(status_map.collect(folders["package"], folders["runtime"], folders["tombox"]))
    svg = tmp_path / "rust.svg"
    ok, out, _, err = rust_map(binary, folders, svg, "--repo", repo, "--emulated-list", registry,
                               "--gpu-coverage", gpu_table)
    assert ok, err
    assert svg.read_text(encoding="utf-8") == python


def test_current_inventory_needs_no_cpp_sources(binary, registry, gpu_table, tmp_path):
    repo = tmp_path / "repo"
    source = repo / "runtime" / "xgameruntime"
    source.mkdir(parents=True)
    for name in ("gdk_api_table.inc", "served_interfaces.inc", "unsupported.txt"):
        (source / name).write_bytes((REPO / "runtime" / "xgameruntime" / name).read_bytes())
    names = translation.not_implemented_functions(source)
    assert "XPackageUninstallChunks" in names and "XTaskQueueRegisterWaiter" not in names
    expected = translation.gdk_runtime_area(source / "gdk_api_table.inc", source / "served_interfaces.inc", source)
    assert expected == translation.gdk_runtime_area()
    folders = make(tmp_path)
    options = ("--emulated-list", registry, "--gpu-coverage", gpu_table)
    original = rust_map(binary, folders, tmp_path / "default.svg", *options)
    copied = rust_map(binary, folders, tmp_path / "copied.svg", *options, "--repo", repo)
    assert original[0] and copied[0], (original[3], copied[3])
    assert copied[1:3] == original[1:3]
    assert f"{expected.done}/{expected.total}".encode() in copied[2]


def test_missing_gdk_tables_are_refused(binary, tmp_path):
    folders = make(tmp_path)
    ok, out, _, err = rust_map(binary, folders, tmp_path / "x.svg", "--repo", tmp_path / "empty")
    assert not ok and out == "" and "gdk_api_table.inc" in err
    assert not (tmp_path / "x.svg").exists()


def test_default_tables_draw_the_map(binary, tmp_path):
    folders = make(tmp_path)
    ok, out, image, err = rust_map(binary, folders, tmp_path / "carte.svg")
    assert ok, err
    assert image.startswith(b'<svg xmlns="http://www.w3.org/2000/svg" width="960"')
    assert out.splitlines()[-1].startswith("GPU RDNA 2 : ")
    assert os.linesep.encode() in image
