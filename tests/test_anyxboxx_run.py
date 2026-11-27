"""`anyxboxx run` (crates/anyxboxx): builds VIctory, then launches it on the user's own game folder.

It replaced main's tools/lancer_victory.ps1. The golden copy of that script (Vice-Tay base) is kept in
tests/fixtures/lancer_victory_vicetay.ps1 (the Neptune-base copy stays beside it, unused): the tests below check that the Rust settings and the
environment `anyxboxx run` gives VIctory are the same as the script's default, -Quality and
-Override behaviour. The launcher runs against a fake repository and game, with a fake cargo on
the PATH and a fake victory.exe already built; the fake tool logs every call and the VICTORY_*
variables it sees.
"""
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[1]
BINARY = REPO / "target" / "release" / "anyxboxx.exe"
GOLDEN = REPO / "tests" / "fixtures" / "lancer_victory_vicetay.ps1"
SETTINGS = REPO / "crates" / "anyxboxx" / "src" / "run" / "settings.rs"
CARGO_FAILURE, VICTORY_EXIT = 101, 7
GAME_DIR = "VICTORY_GAME_DIR=<root>\\game"
BUILD = r"cargo build --release -p victory --manifest-path <root>\depot\third_party\victory\Cargo.toml | "
BUILD_DIAG = (r"cargo build --profile diag -p victory --features diagnostics "
              r"--manifest-path <root>\depot\third_party\victory\Cargo.toml | ")
GAME = r"victory run <root>\game\game_scarlett_master.exe --execute | "
GAME_DIAG = r"victory run <root>\game\game_scarlett_master.exe --execute | "
GAME_ARCHIVES = r"<root>\game\A.rpf <root>\game\b.rpf"
# Vice-Tay's `warm` takes no files: it remakes the pipelines earlier runs noted.
WARM_ARCHIVES = r"victory warm | "
WARM_PRECOMPILE = r"victory warm | "


# ---- the golden script and the Rust settings ------------------------------------------------

def _golden_pairs(block_start, block_end, pattern):
    text = GOLDEN.read_text(encoding="utf-8")
    body = text[text.index(block_start):]
    body = body[:body.index(block_end)]
    return re.findall(pattern, body)


def golden_tuned():
    """The `$tuned = [ordered]@{ ... }` table of the script, in order."""
    return _golden_pairs("$tuned = [ordered]@{", "\n    }", r"(VICTORY_[A-Z0-9_]+)\s*=\s*'([^']*)'")


def golden_quality():
    return _golden_pairs("if ($Quality) {", "\n}", r"\$env:(VICTORY_[A-Z0-9_]+) = '([^']*)'")


def golden_always():
    return _golden_pairs("$env:VICTORY_GAME_DIR = $GameDir", "if (-not $NoTunedSettings)",
                         r"\$env:(VICTORY_[A-Z0-9_]+) = '([^']*)'")


def rust_table(name):
    text = SETTINGS.read_text(encoding="utf-8")
    start = text.index(f"pub const {name}")
    body = text[start:text.index("];", start)]
    return re.findall(r'\(\s*"(VICTORY_[A-Z0-9_]+)",\s*"([^"]*)",?\s*\)', body)


def test_the_rust_tuned_table_is_the_golden_scripts_table():
    assert rust_table("TUNED") == golden_tuned()
    assert len(golden_tuned()) == 26


def test_the_rust_quality_set_is_the_golden_scripts_quality_set():
    assert rust_table("QUALITY") == golden_quality()
    assert [k for k, _ in golden_quality()] == ["VICTORY_CARD_WINDOWS_MB", "VICTORY_CARD_BUFFERS_MB",
                                                "VICTORY_TEXTURE_STAGE_MB"]


def test_the_always_set_is_the_golden_scripts_discord_switch():
    assert rust_table("ALWAYS") == golden_always() == [("VICTORY_DISCORD", "0")]


def default_env(extra=()):
    """The environment `anyxboxx run` gives VIctory by default, as the fake tool logs it (sorted)."""
    pairs = [GAME_DIR] + [f"{k}={v}" for k, v in golden_always() + golden_tuned()] + list(extra)
    return ";".join(sorted(pairs))


# ---- the fake tools ---------------------------------------------------------------------------

# Fake console tool (.NET Framework, compiled by Windows PowerShell), copied as cargo.exe and victory.exe. Its exit
# code comes from FAKE_TOOL_EXIT_<NAME>_<FIRST ARGUMENT>, else FAKE_TOOL_EXIT_<NAME>, else 0.
FAKE_TOOL = r"""
using System; using System.IO; using System.Linq; using System.Collections;
public static class FakeTool {
    public static int Main(string[] args) {
        string name = Path.GetFileNameWithoutExtension(System.Reflection.Assembly.GetEntryAssembly().Location);
        var env = Environment.GetEnvironmentVariables().Cast<DictionaryEntry>()
            .Select(e => (string)e.Key).Where(k => k.StartsWith("VICTORY_")).OrderBy(k => k, StringComparer.Ordinal)
            .Select(k => k + "=" + Environment.GetEnvironmentVariable(k));
        File.AppendAllText(Environment.GetEnvironmentVariable("FAKE_TOOL_LOG"),
                           name + " " + string.Join(" ", args) + " | " + string.Join(";", env) + Environment.NewLine);
        string key = "FAKE_TOOL_EXIT_" + name.ToUpperInvariant();
        string code = Environment.GetEnvironmentVariable(args.Length > 0 ? key + "_" + args[0].ToUpperInvariant() : key)
                      ?? Environment.GetEnvironmentVariable(key);
        return string.IsNullOrEmpty(code) ? 0 : int.Parse(code);
    }
}
"""


@pytest.fixture(scope="module")
def fake_tool(tmp_path_factory):
    if sys.platform != "win32" or not shutil.which("powershell"):
        pytest.skip("the fake tools are .NET programs compiled by Windows PowerShell")
    folder = tmp_path_factory.mktemp("fake-tool")
    source, exe = folder / "FakeTool.cs", folder / "FakeTool.exe"
    source.write_text(FAKE_TOOL, encoding="utf-8")
    subprocess.run(["powershell", "-NoProfile", "-Command",
                    f"Add-Type -Path '{source}' -OutputAssembly '{exe}' -OutputType ConsoleApplication"],
                   check=True, capture_output=True, timeout=120)
    return exe


@pytest.fixture(scope="module")
def binary():
    if not BINARY.is_file():
        if os.environ.get("ANYXBOXX_REQUIRE_RUNTIME") == "1":
            pytest.fail(f"{BINARY} absent: cargo build --release (root workspace).")
        pytest.skip("anyxboxx not built (cargo build --release at the root)")
    return BINARY


def launch(tmp_path, fake_tool, binary, options=(), *, cargo_exit=0, game_exe=True, precompile_inputs=False,
           archives=("A.rpf", "b.rpf"), logical_cpus=8):
    """Runs `anyxboxx run` in a fake repository; returns (process, calls with <root> for tmp_path)."""
    manifest = tmp_path / "depot" / "third_party" / "victory" / "Cargo.toml"
    manifest.parent.mkdir(parents=True)
    manifest.write_text("", encoding="utf-8")
    game = tmp_path / "game"
    game.mkdir()
    if game_exe:
        (game / "game_scarlett_master.exe").write_bytes(b"")
    for name in archives:
        (game / name).write_bytes(b"")
    if precompile_inputs:
        for name in ("shaders.rpf", "XboxBvhBuilder.dll", "amd_fidelityfx_upscaler_xbox.dll"):
            (game / name).write_bytes(b"")
    for folder in ("target/release", "target/diag"):
        (tmp_path / folder).mkdir(parents=True)
        shutil.copy(fake_tool, tmp_path / folder / "victory.exe")
    (tmp_path / "tools").mkdir()
    shutil.copy(fake_tool, tmp_path / "tools" / "cargo.exe")
    (tmp_path / "xg_xs.dll").write_bytes(b"")
    (tmp_path / "prebuilt").mkdir()
    shutil.copy(fake_tool, tmp_path / "prebuilt" / "prebuilt.exe")
    log = tmp_path / "calls.log"
    env = {k: v for k, v in os.environ.items() if not k.startswith("VICTORY_")}
    # The golden set is the 8-core one: pin the CPU count so the result does not depend on this machine.
    env["ANYXBOXX_LOGICAL_CPUS"] = str(logical_cpus)
    env.update({"PATH": f"{tmp_path / 'tools'}{os.pathsep}{os.environ['PATH']}",
                "CARGO_TARGET_DIR": str(tmp_path / "target"), "FAKE_TOOL_LOG": str(log),
                "FAKE_TOOL_EXIT_CARGO": str(cargo_exit), "FAKE_TOOL_EXIT_VICTORY_RUN": str(VICTORY_EXIT),
                "FAKE_TOOL_EXIT_PREBUILT_RUN": str(VICTORY_EXIT)})
    options = [o.replace("{xg}", str(tmp_path / "xg_xs.dll")).replace("{prebuilt}", str(tmp_path / "prebuilt"))
               for o in options]
    command = [str(binary), "run", "--repo", str(tmp_path / "depot"), "--game-dir", str(game), *options]
    done = subprocess.run(command, env=env, capture_output=True, text=True, timeout=120)
    calls = log.read_text(encoding="utf-8").splitlines() if log.is_file() else []
    return done, [call.replace(str(tmp_path), "<root>") for call in calls]


# ---- launches ---------------------------------------------------------------------------------

def test_default_launch_is_the_release_build_then_the_game_with_the_golden_environment(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary)
    assert calls == [BUILD, GAME + default_env()], done.stderr
    assert done.returncode == VICTORY_EXIT


def test_the_default_environment_equals_the_scripts_default(tmp_path, fake_tool, binary):
    # The same set as main's script with no switch: the script's always-set pair, then its tuned set.
    done, calls = launch(tmp_path, fake_tool, binary)
    logged = calls[1].split(" | ", 1)[1].split(";")
    expected = ["VICTORY_GAME_DIR=<root>\\game"] + [f"{k}={v}" for k, v in golden_always() + golden_tuned()]
    assert sorted(logged) == sorted(expected), done.stderr


def test_sixteen_or_more_logical_cpus_get_the_bigger_worker_pools(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, logical_cpus=24)
    logged = calls[1].split(" | ", 1)[1].split(";")
    for pair in ("VICTORY_THIN_THREADS=12", "VICTORY_UNDER_THREADS=8", "VICTORY_LAND_THREADS=8"):
        assert pair in logged, (pair, done.stderr)
    user = tmp_path / "user"
    user.mkdir()
    done, calls = launch(user, fake_tool, binary, ["--override", "VICTORY_LAND_THREADS=5"], logical_cpus=24)
    assert "VICTORY_LAND_THREADS=5" in calls[1].split(" | ", 1)[1].split(";"), done.stderr


def test_quality_applies_the_golden_quality_set_after_the_tuned_set(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--quality"])
    logged = calls[1].split(" | ", 1)[1].split(";")
    for k, v in golden_quality():
        assert f"{k}={v}" in logged, (k, done.stderr)
    # The tuned value of a key Quality does not replace is still there.
    assert "VICTORY_RELAY=2" in logged, done.stderr


def test_dred_asked_for_replaces_the_tuned_light_value(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--dred"])
    logged = calls[1].split(" | ", 1)[1].split(";") if len(calls) > 1 else calls[0].split(" | ", 1)[1].split(";")
    assert "VICTORY_D3D12_DRED=1" in logged and "VICTORY_D3D12_DRED=light" not in logged, done.stderr


def test_the_default_launch_carries_dred_light(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary)
    assert "VICTORY_D3D12_DRED=light" in calls[1].split(" | ", 1)[1].split(";"), done.stderr


def test_override_wins_over_quality_and_the_tuned_set(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--quality", "--override", "VICTORY_RELAY=0"])
    logged = calls[1].split(" | ", 1)[1].split(";")
    assert "VICTORY_RELAY=0" in logged and "VICTORY_RELAY=2" not in logged, done.stderr


def test_auto_a_and_frametime_dump_are_set_when_asked(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--auto-a", "--frametime-dump"])
    logged = calls[1].split(" | ", 1)[1].split(";")
    assert "VICTORY_AUTO_A=150" in logged and "VICTORY_FRAMETIME_DUMP=1" in logged, done.stderr


def test_opt_in_switches_and_no_tuned_settings(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary,
                         ["--no-tuned-settings", "--dred", "--batch-card-rehash", "--skip-pipelines", "5"])
    expected = (GAME_DIR, "VICTORY_DISCORD=0", "VICTORY_CARD_REHASH_RUNS=1",
                "VICTORY_D3D12_DRED=1", "VICTORY_SKIP_PIPELINES=5")
    assert calls == [BUILD_DIAG, GAME + ";".join(sorted(expected))], done.stderr


def test_diag_builds_the_diagnostics_profile(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--diag", "--no-tuned-settings"])
    assert calls[0] == BUILD_DIAG, done.stderr


def test_the_users_xg_library_is_passed_with_its_mode(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--no-tuned-settings", "--xg-dll", "{xg}", "--xg-mode", "compare"])
    expected = (GAME_DIR, "VICTORY_DISCORD=0",
                r"VICTORY_XG_DLL=<root>\xg_xs.dll", "VICTORY_XG_MODE=compare")
    assert calls == [BUILD, GAME + ";".join(sorted(expected))], done.stderr


def test_a_missing_xg_library_stops_after_the_build(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--xg-dll", "missing.dll"])
    assert calls == [BUILD] and done.returncode == 1
    assert "xg_xs.dll not found" in done.stderr


def test_warm_runs_the_game_archives_only_before_the_game(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--warm"])
    assert calls == [BUILD, WARM_ARCHIVES + default_env(), GAME + default_env()], done.stderr


def test_warm_needs_an_archive(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--warm"], archives=())
    assert calls == [BUILD] and done.returncode == 1
    assert "no .rpf archive" in done.stderr


def test_precompile_warms_the_shader_archive_and_binaries_with_the_game_environment(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--no-tuned-settings", "--precompile"],
                         precompile_inputs=True)
    env = ";".join(sorted((GAME_DIR, "VICTORY_DISCORD=0")))
    assert calls == [BUILD, WARM_PRECOMPILE + env, GAME + env], done.stderr
    assert done.returncode == VICTORY_EXIT


def test_precompile_needs_the_shader_archive(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--precompile"])
    assert calls == [BUILD] and done.returncode == 1
    assert "shaders.rpf" in done.stderr


def test_a_failed_build_stops_the_launch(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, cargo_exit=CARGO_FAILURE)
    assert calls == [BUILD] and done.returncode == 1
    assert f"cargo build failed (exit code {CARGO_FAILURE})" in done.stderr


def test_a_missing_game_stops_before_building(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, game_exe=False)
    assert calls == [] and done.returncode == 1
    assert "game_scarlett_master.exe not found" in done.stderr


def test_powershell_option_names_still_work(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--NoTunedSettings", "--Dred"])
    expected = (GAME_DIR, "VICTORY_DISCORD=0", "VICTORY_D3D12_DRED=1")
    assert calls == [BUILD_DIAG, GAME + ";".join(sorted(expected))], done.stderr


# Linux (Proton): there is no cargo inside Proton, so VIctory is cross-built on the Linux side and
# `--victory-exe` runs that build as is. The fake build is named prebuilt.exe, so its calls log as
# `prebuilt`, apart from the repository's victory.exe; `cargo.exe` stays on the PATH, so any build
# call would be logged too.
PREBUILT_GAME = GAME.replace("victory run", "prebuilt run", 1)


def test_a_prebuilt_victory_is_run_without_building(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--victory-exe", r"{prebuilt}\prebuilt.exe"])
    assert calls == [PREBUILT_GAME + default_env()], done.stderr
    assert done.returncode == VICTORY_EXIT


def test_a_missing_prebuilt_victory_stops_before_anything_runs(tmp_path, fake_tool, binary):
    done, calls = launch(tmp_path, fake_tool, binary, ["--victory-exe", r"{prebuilt}\missing.exe"])
    assert calls == [] and done.returncode == 1
    assert "victory.exe not found" in done.stderr
