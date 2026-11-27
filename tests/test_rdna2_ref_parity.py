"""Parité de l'interpréteur de référence Rust (crates/anyxboxx-rdna2-ref) avec emu/rdna2_exec.py.

Les mêmes programmes passent par les deux interpréteurs ; l'état final (pc, scc, registres
scalaires et vectoriels, nombre d'instructions exécutées) doit être identique, ligne par ligne.
Les programmes aléatoires (graine fixe) couvrent les familles scalaires SOP1, SOP2, SOPC, SOPK et
SOPP, en wave32 comme en wave64. Une différence est un bug de l'un des deux interpréteurs.

Le binaire est `target/release/anyxboxx-rdna2-ref(.exe)` (ou `target/debug`) ; absent, le test est
sauté, sauf en CI (ANYXBOXX_REQUIRE_RUNTIME=1).
"""
import os
import random
import re
import struct
import subprocess
from pathlib import Path

import pytest

from emu import rdna2_exec, rdna2_image
from emu.rdna2_exec import VOP2_CARRY, Wave
from emu.rdna2_exec_cmp64 import COMPARE64
from emu.rdna2_exec_dot import OPERATIONS as DOT_OPERATIONS
from emu.rdna2_exec_f16 import OPERATIONS as F16_OPERATIONS
from emu.rdna2_exec_int16 import PACKED
from emu.rdna2_exec_pkf16 import PACKED_F16, VOP3P as PKF16_VOP3P
from emu.rdna2_exec_memory import PAGE
from emu.rdna2_exec_movrel import SCALAR as MOVREL_SCALAR, VECTOR as MOVREL_VECTOR
from emu.rdna2_exec_wide import OPERATIONS as WIDE_OPERATIONS
from emu.rdna2_exec_vop3 import VECTOR3
from emu.rdna2_exec_ops import BRANCHES, NO_OPERATION, SCALAR, VECTOR, VECTOR_COMPARE, VECTOR_NO_OPERATION
from emu.rdna2_ops import MNEMONICS

REPO = Path(__file__).resolve().parents[1]
GENERATED = REPO / "crates" / "anyxboxx-rdna2-ref" / "src" / "mnemonics.rs"
OPCODES = {(family, name): opcode for (family, opcode), name in MNEMONICS.items()}
SCALAR_FAMILIES = ("SOP1", "SOP2", "SOPC", "SOPK", "SOPP")
# Opérandes sources que l'interpréteur sait lire : SGPR, constantes, flottants en ligne, littéral.
OPERANDS = [*range(0, 128), *range(128, 209), *range(240, 249), 255]
LITERAL = 255
SEED = 0x5CA1AB1E
PROGRAMS = 300


def binary():
    for profile in ("release", "debug"):
        for name in ("anyxboxx-rdna2-ref.exe", "anyxboxx-rdna2-ref"):
            path = REPO / "target" / profile / name
            if path.is_file():
                return path
    override = os.environ.get("ANYXBOXX_RDNA2_REF")
    if override and Path(override).is_file():
        return Path(override)
    if os.environ.get("ANYXBOXX_REQUIRE_RUNTIME") == "1":
        pytest.fail("anyxboxx-rdna2-ref absent : cargo build -p anyxboxx-rdna2-ref")
    pytest.skip("anyxboxx-rdna2-ref not built (cargo build -p anyxboxx-rdna2-ref)")


def sop2(name, sdst, ssrc0, ssrc1):
    return 0b10 << 30 | OPCODES["SOP2", name] << 23 | sdst << 16 | ssrc1 << 8 | ssrc0


def sop1(name, sdst, ssrc0):
    return 0x17D << 23 | sdst << 16 | OPCODES["SOP1", name] << 8 | ssrc0


def sopc(name, ssrc0, ssrc1):
    return 0x17E << 23 | OPCODES["SOPC", name] << 16 | ssrc1 << 8 | ssrc0


def sopk(name, sdst, simm16):
    return 0b1011 << 28 | OPCODES["SOPK", name] << 23 | sdst << 16 | simm16 & 0xFFFF


def sopp(name, simm16=0):
    return 0x17F << 23 | OPCODES["SOPP", name] << 16 | simm16 & 0xFFFF


def program(*words):
    """Programme terminé par s_endpgm, sous forme de mots (ce qu'attend le binaire Rust)."""
    return [*words, sopp("s_endpgm")]


def render(wave):
    """L'état d'un Wave, au format de la sortie de anyxboxx-rdna2-ref."""
    lines = [f"steps {wave.steps}", f"pc {wave.pc}", f"scc {int(wave.scc)}"]
    lines += [f"s {index} {value:08X}" for index, value in enumerate(wave.s) if value]
    for index, lanes in enumerate(wave.v[:256]):
        if any(lanes):
            lines.append(f"v {index} " + " ".join(f"{value:08X}" for value in lanes))
    for page, data in sorted(wave.memory.pages.items()):
        words = struct.unpack("<1024I", bytes(data))
        entries = "".join(f" {i * 4:X}:{value:08X}" for i, value in enumerate(words) if value)
        lines.append(f"page {page * PAGE:016X}{entries}")
    for target, exported in sorted(getattr(wave, "exports", {}).items()):
        for lane, values in sorted(exported.items()):
            lines.append(f"exp {target} {lane} " + " ".join(f"{value:08X}" for value in values))
    return "\n".join(lines) + "\n"


def run_python(lanes, scc, scalars, words, memory=(), primitive=0, primitives=None):
    wave = Wave(lanes=lanes)
    wave.exports = {}                   # EXP : comme run_vertex et run_pixel_wave (emu/rdna2_exec_graphics.py)
    wave.primitive, wave.primitives = primitive, primitives       # VINTRP : primitive commune, ou par ligne
    wave.scc = scc
    for index, value in scalars:
        wave.s[index] = value
    for address, values in memory:
        wave.memory.write_words(address, values)
    data = struct.pack(f"<{len(words)}I", *words)
    try:
        rdna2_exec.run(data, wave)
    except rdna2_exec.Untranslated:
        return "error untranslated\n"
    except RuntimeError:
        return "error loop\n"
    return render(wave)


def run_rust(exe, lanes, scc, scalars, words, memory=(), primitive=0, primitives=None):
    lines = [f"lanes {lanes}", f"scc {int(scc)}", f"primitive {primitive}"]
    if primitives is not None:
        lines.append("primitives " + " ".join(f"{lane}:{value}" for lane, value in primitives.items()))
    lines += [f"s {index} {value:X}" for index, value in scalars]
    lines += [f"mem {address:X} " + " ".join(f"{value:08X}" for value in values) for address, values in memory]
    lines.append("program " + " ".join(f"{word:08X}" for word in words))
    done = subprocess.run(
        [str(exe)], input="\n".join(lines) + "\n", capture_output=True, text=True, encoding="utf-8", timeout=60
    )
    assert done.returncode == 0, done.stderr
    return done.stdout


def assert_same(exe, lanes, scc, scalars, words, memory=(), **graphics):
    rust = run_rust(exe, lanes, scc, scalars, words, memory, **graphics)
    python = run_python(lanes, scc, scalars, words, memory, **graphics)
    assert rust == python, f"lanes={lanes} scc={scc} scalars={scalars} memory={memory} graphics={graphics}\nprogram={[hex(w) for w in words]}\n" \
                           f"rust:\n{rust}\npython:\n{python}"


def test_the_generated_mnemonic_table_matches_the_python_one():
    text = GENERATED.read_text(encoding="utf-8")
    found = {
        (family, int(opcode, 16)): name
        for family, opcode, name in re.findall(r'\("(\w+)", 0x([0-9A-F]+)\) => "(\w+)"', text)
    }
    assert found == MNEMONICS


def test_fixed_scalar_programs_match_the_python_model():
    exe = binary()
    cases = [
        (32, False, [(0, 0xFFFF_FFFF), (1, 1)], program(sop2("s_add_u32", 2, 0, 1), sop2("s_addc_u32", 3, 0, 1))),
        (32, False, [(0, 0x7FFF_FFFF), (1, 1)], program(sop2("s_add_i32", 2, 0, 1), sop2("s_mul_i32", 3, 0, 1))),
        (32, True, [(0, 5), (1, 9)], program(sop2("s_cselect_b32", 2, 0, 1), sop2("s_cselect_b64", 4, 0, 1))),
        (64, False, [(0, 1), (2, 3)], program(sop2("s_lshl_b64", 4, 0, 2), sop2("s_bfm_b64", 6, 0, 2))),
        (32, False, [(0, 0x12), (1, 7)], program(sop2("s_lshl1_add_u32", 2, 0, 1), sop2("s_bfe_u32", 3, 0, 1))),
        (32, False, [(0, 0xABCD_1234)], program(sop1("s_brev_b32", 1, 0), sop1("s_flbit_i32_b32", 2, 0))),
        (32, False, [(0, 0x80)], program(sop1("s_sext_i32_i8", 1, 0), sop1("s_abs_i32", 2, 1))),
        (32, False, [(0, 0xF0F0), (126, 0xFF)], program(sop1("s_and_saveexec_b32", 3, 0), sop1("s_wqm_b32", 4, 0))),
        (32, False, [(0, 1)], program(sopc("s_cmp_lt_i32", 0, 0), sopk("s_movk_i32", 5, 0xFFFE))),
        (32, False, [(7, 9)], program(sopk("s_addk_i32", 7, 100), sopk("s_cmpk_eq_u32", 7, 109))),
        (32, False, [], program(sopp("s_branch", 1), sop1("s_mov_b32", 0, 255), sop1("s_mov_b32", 1, 130))),
    ]
    for lanes, scc, scalars, words in cases:
        assert_same(exe, lanes, scc, scalars, words)


V0 = 256                        # v0 as a source operand (9 bits)
ONE_F, TWO_F = 242, 244         # inline constants 1.0 and 2.0


def vop1(name, vdst, src0):
    return 0x3F << 25 | vdst << 17 | OPCODES["VOP1", name] << 9 | src0


def vop2(name, vdst, src0, vsrc1):
    return OPCODES["VOP2", name] << 25 | vdst << 17 | vsrc1 << 9 | src0


def test_fixed_vector_programs_match_the_python_model():
    exe = binary()
    cases = [
        (32, False, [], program(vop1("v_cvt_f32_u32", 1, V0), vop2("v_mul_f32", 1, TWO_F, 1),
                                vop2("v_add_f32", 1, ONE_F, 1))),
        (32, False, [], program(vop2("v_add_co_ci_u32", 2, V0, V0), vop2("v_sub_co_ci_u32", 3, V0, 2))),
        (64, True, [], program(vop2("v_add_co_ci_u32", 2, V0, V0), vop1("v_readfirstlane_b32", 5, V0))),
        (32, False, [], program(vop1("v_sqrt_f32", 1, 255), 0xC080_0000, vop2("v_min_f32", 2, 255, 1), 0x7FC0_0000)),
        (32, False, [], [vop2("v_madmk_f32", 1, V0, 2), 0x4000_0000, sopp("s_endpgm")]),
        (32, False, [], program(vop1("v_fract_f32", 1, 255), 0xFF80_0000, vop1("v_rcp_f32", 2, 128))),
        (32, False, [], program(vop1("v_cvt_flr_i32_f32", 1, 255), 0x7FC0_0000)),
    ]
    for lanes, scc, scalars, words in cases:
        assert_same(exe, lanes, scc, scalars, words)


# Operand values of the unfused multiply-add test: a finite value, +inf, +0, and a NaN whose payload
# names its position (0A, 0B, 0C), so that the NaN that comes out says which operand it was.
MUL_ADD_NANS = (0x7FC0_0A0A, 0x7FC0_0B0B, 0x7FC0_0C0C)
MUL_ADD_VALUES = (0x3FC0_0000, 0x7F80_0000, 0x0000_0000)


def mul_add_programs():
    """(name, words) of every unfused multiply-add form with each operand position finite, infinite, zero
    or NaN. The model's `_mul_add` keeps the NaN of the product (a factor, or ∞ × 0) before a NaN addend."""
    load = lambda register, bits: [vop1("v_mov_b32", register, LITERAL), bits]
    programs = []
    for first in (*MUL_ADD_VALUES, MUL_ADD_NANS[0]):
        for second in (*MUL_ADD_VALUES, MUL_ADD_NANS[1]):
            for third in (*MUL_ADD_VALUES, MUL_ADD_NANS[2]):
                registers = load(1, first) + load(2, second) + load(3, third)
                programs += [
                    ("v_mad_f32", program(*registers, *vop3("v_mad_f32", 4, (V0 + 1, V0 + 2, V0 + 3)))),
                    ("v_mac_f32", program(*registers, vop2("v_mac_f32", 3, V0 + 1, 2))),
                    ("v_mac_f32 (VOP3)", program(*registers, *vop3("v_mac_f32", 3, (V0 + 1, V0 + 2, 128)))),
                    ("v_madak_f32", program(*load(1, first) + load(2, second), vop2("v_madak_f32", 4, V0 + 1, 2), third)),
                    ("v_madmk_f32", program(*load(1, first) + load(2, third), vop2("v_madmk_f32", 4, V0 + 1, 2), second)),
                ]
    return programs


def test_unfused_multiply_add_nans_match_the_python_model():
    """v_mad_f32, v_mac_f32 (VOP2 and VOP3), v_madak_f32 and v_madmk_f32 on every mix of finite, infinite,
    zero and NaN operands: the same NaN as the model, which keeps the product's NaN before the addend's."""
    exe = binary()
    for _, words in mul_add_programs():
        assert_same(exe, 32, False, [], words)
    # The values themselves, so that two models wrong in the same way cannot pass.
    madak = program(vop1("v_mov_b32", 1, LITERAL), 0x3FC0_0000, vop1("v_mov_b32", 2, LITERAL), MUL_ADD_NANS[1],
                    vop2("v_madak_f32", 4, V0 + 1, 2), MUL_ADD_NANS[2])
    assert "\nv 4 7FC00B0B" in run_python(32, False, [], madak)
    infinity_times_zero = program(vop1("v_mov_b32", 1, LITERAL), 0x7F80_0000, vop1("v_mov_b32", 3, LITERAL),
                                  MUL_ADD_NANS[2], *vop3("v_mad_f32", 4, (V0 + 1, V0 + 2, V0 + 3)))
    assert "\nv 4 FFC00000" in run_python(32, False, [], infinity_times_zero)


def test_a_literal_operand_is_the_second_word_in_both_models():
    exe = binary()
    words = program(sop1("s_mov_b32", 0, 255), 0xDEAD_BEEF, sop2("s_xor_b32", 1, 0, 255), 0x0F0F_0F0F)
    assert_same(exe, 32, False, [], words)


def test_inline_double_constants_match_the_python_model():
    exe = binary()
    for lanes in (32, 64):
        for source in range(240, 249):
            for instruction in (sop1("s_mov_b64", 4, source), sop1("s_not_b64", 4, source),
                                sop2("s_xor_b64", 4, source, 193)):
                assert_same(exe, lanes, False, [], program(instruction))
            assert_same(exe, lanes, False, [], program(vopc("v_cmp_eq_f64", source, 0)))


def test_signed_64_bit_sources_sign_extend_a_literal_in_both_models():
    """ISA section 6.2.1 (p. 43): a 32-bit literal is sign-extended for a signed 64-bit source (s_ashr_i64,
    s_bfe_i64, s_flbit_i32_i64) and zero-extended for the unsigned and bitwise ones."""
    exe = binary()
    control = [(60, 40 << 16 | 4)]                                   # s_bfe: offset 4, width 40
    for value in (0x8000_0000, 0xFFFF_FFF0, 0xFACE_5766, 0x7FFF_FFFF, 0):
        for words in (
            program(sop2("s_ashr_i64", 40, LITERAL, 129), value), program(sop2("s_ashr_i64", 40, LITERAL, 132), value),
            program(sop2("s_bfe_i64", 44, LITERAL, 60), value), program(sop2("s_bfe_u64", 46, LITERAL, 60), value),
            program(sop1("s_flbit_i32_i64", 51, LITERAL), value), program(sop1("s_flbit_i32_b64", 52, LITERAL), value),
            program(sop1("s_mov_b64", 48, LITERAL), value),
        ):
            assert_same(exe, 32, False, control, words)
    # The values themselves, so that two models wrong in the same way cannot pass.
    assert "s 51 0000003C\n" in run_python(32, False, [], program(sop1("s_flbit_i32_i64", 51, LITERAL), 0xFFFF_FFF0))
    assert "s 52 00000020\n" in run_python(32, False, [], program(sop1("s_flbit_i32_b64", 52, LITERAL), 0xFFFF_FFF0))


def test_scalar_packs_match_the_python_model():
    exe = binary()
    for name in ("s_pack_ll_b32_b16", "s_pack_lh_b32_b16", "s_pack_hh_b32_b16"):
        for scc in (False, True):
            for a, b in ((0, 1), (240, 248), (193, 192), (LITERAL, 1), (0, LITERAL)):
                words = [sop2(name, 3, a, b)]
                if LITERAL in (a, b):
                    words.append(0xAAAA_BBBB)
                assert_same(exe, 32, scc, [(0, 0x12345678), (1, 0x9abcdef0)], program(*words))


def test_sdwa_integer_output_modifiers_match_the_python_model():
    exe = binary()
    instruction = vop2("v_and_b32", 3, 0xF9, 0)
    for lanes in (32, 64):
        for flags in (0, 1 << 13, 1 << 14, 2 << 14, 3 << 14):
            program_words = program(instruction, 6 << 8 | 6 << 16 | 6 << 24 | flags)
            assert_same(exe, lanes, False, [], program_words)


# Float literals the model treats specially: NaN, infinities, signed zeros, subnormals, the largest finite
# value, one. Half the literals come from this list, the rest are random 32-bit patterns.
SPECIAL_LITERALS = (
    0x7FC0_0000, 0xFFC0_0000, 0x7F80_0000, 0xFF80_0000, 0x0000_0000, 0x8000_0000, 0x0000_0001,
    0x8000_0001, 0x7F7F_FFFF, 0x3F80_0000, 0xC080_0000, 0x3F00_0000, 0x4000_0000,
)
VECTOR_FAMILIES = ("VOP1", "VOP2")
SAD_NAMES = ("v_msad_u8", "v_qsad_pk_u16_u8", "v_mqsad_pk_u16_u8", "v_mqsad_u32_u8")
# SDWA (0xF9), DPP16 (0xFA), DPP8 (0xE9) and DPP8_FI (0xEA): the source codes that announce a second word.
EXT_CODES = (0xF9, 0xFA, 0xE9, 0xEA)
EXT_PROBABILITY = 0.3


def extension_word(rng, code, vsrc1):
    """A random SDWA or DPP word whose scalar sources are readable: no SGPR read of the literal code 255,
    and no scalar read of a VOP2 vsrc1 that is the literal code (both are Python TypeErrors)."""
    word = rng.getrandbits(32)
    if code == 0xF9:
        if word & 0xFF == LITERAL:
            word &= ~(1 << 23)
        if vsrc1 == LITERAL:
            word &= ~(1 << 31)
    return word


def literal_bits(rng):
    return rng.choice(SPECIAL_LITERALS) if rng.random() < 0.5 else rng.getrandbits(32)
# Literal-carrying VOP2 opcodes (v_madmk, v_madak, v_fmamk, v_fmaak, f32 and f16): the literal is always there.
VOP2_WITH_LITERAL = {0x20, 0x21, 0x2C, 0x2D, 0x37, 0x38}


def random_program(rng):
    """Instructions scalaires et vectorielles aléatoires, valides pour les deux modèles.

    Les branches sautent vers le début d'une instruction plus loin dans le programme (ou vers s_endpgm) :
    sans cela un saut tombe dans un mot de données, qui se décode en instruction mémoire (hors du modèle)."""
    names = {family: [] for family in (*SCALAR_FAMILIES, *VECTOR_FAMILIES, "VOPC", "VOP3", "VOP3P")}
    for (family, opcode), name in MNEMONICS.items():
        if family in SCALAR_FAMILIES and name != "s_endpgm" and (
            name in MOVREL_SCALAR or
            name in SCALAR or name in BRANCHES or name in NO_OPERATION
        ):
            names[family].append((opcode, name))
        # Native VOP3 names at opcodes 0x200 and up: the Python routes lower opcodes through the VOP1/VOP2 tables.
        if family == "VOP3" and (
            (name in VECTOR3 and opcode >= 0x200)
            or name in SAD_NAMES
            or (name in VECTOR and (0x100 <= opcode < 0x140 or 0x180 <= opcode < 0x200))
            or (name in VECTOR_COMPARE and opcode < 0x100)
            or name in VOP3_CARRY + VOP3_MAD64 + VOP3_LANE + DIV_NAMES
            or name in COMPARE64
            or name in WIDE_GENERIC
            or name in MOVREL_VECTOR
            or name in F16_ARITH_NAMES
        ):
            names[family].append((opcode, name))
        if family == "VOP1" and (name in WIDE_GENERIC or name in MOVREL_VECTOR):
            names[family].append((opcode, name))
        if family == "VOP3P" and name in DOT_OPERATIONS:
            names[family].append((opcode, name))
        if family == "VOPC" and (name in VECTOR_COMPARE or name in COMPARE64):
            names[family].append((opcode, name))
        if family in VECTOR_FAMILIES and (
            name in VECTOR or name in VECTOR_NO_OPERATION or name in VOP2_CARRY or name in F16_ARITH_NAMES
        ):
            names[family].append((opcode, name))
    blocks = []  # (words, branch template or None): a branch's offset is patched once the layout is known
    for _ in range(rng.randrange(1, 11)):
        words, template = instruction(rng, names)
        blocks.append((words, template))
    starts, position = [], 0
    for words, _ in blocks:
        starts.append(position)
        position += len(words)
    program_words = []
    for index, (words, template) in enumerate(blocks):
        if template is not None:
            # Target: a later instruction start, or s_endpgm (the position after the last block).
            later = [starts[j] for j in range(index + 1, len(blocks))] + [position]
            target = rng.choice(later[:3])
            words = [template | (target - starts[index] - 1)]
        program_words += words
    return program_words


def instruction(rng, names):
    """One random instruction: (its words, the branch template when it is a branch)."""
    family = rng.choice((*SCALAR_FAMILIES, *VECTOR_FAMILIES, "VOPC", "VOP3", "VOP3P"))
    opcode, name = rng.choice(names[family])
    ssrc0, ssrc1 = rng.choice(OPERANDS), rng.choice(OPERANDS)
    # 64-bit results need the register after sdst: stay below s126 (s127 is the last).
    sdst = rng.randrange(126)
    if family == "SOPP" and name in BRANCHES:
        return [0], 0x17F << 23 | opcode << 16
    if family == "SOP2":
        word = sop2_word(opcode, sdst, ssrc0, ssrc1)
    elif family == "SOP1":
        if name in MOVREL_SCALAR:
            ssrc0 = rng.randrange(106)                               # an indexed SGPR source (s0 to s105)
        word = 0x17D << 23 | sdst << 16 | opcode << 8 | ssrc0
    elif family == "SOPC":
        word = 0x17E << 23 | opcode << 16 | ssrc1 << 8 | ssrc0
    elif family == "SOPK":
        word = 0b1011 << 28 | opcode << 23 | sdst << 16 | rng.getrandbits(16)
    elif family == "SOPP":
        word = 0x17F << 23 | opcode << 16 | rng.randrange(0, 3)
    elif family == "VOP1":
        # v_readfirstlane_b32 writes an SGPR through the vdst field: keep it below s128.
        vdst = rng.randrange(128) if name == "v_readfirstlane_b32" else rng.randrange(255 if name in WIDE_GENERIC else 256)
        if name in MOVREL_VECTOR:
            ssrc0 = rng.randrange(256, 511)                          # an indexed VGPR source
        word = 0x3F << 25 | vdst << 17 | opcode << 9 | ssrc0
    elif family == "VOP3":
        return vop3_words(rng, opcode, name)
    elif family == "VOP3P":
        return vop3p_words(rng, opcode)
    elif family == "VOPC":
        # A VGPR pair as source 0 (64-bit compares read v[n], v[n+1]): n + 1 stays below v256.
        ssrc0 = rng.choice(OPERANDS + list(range(256, 511)))
        word = 0x3E << 25 | opcode << 17 | rng.randrange(255) << 9 | ssrc0
    else:
        vdst, vsrc1 = rng.randrange(256), rng.randrange(256)
        word = opcode << 25 | vdst << 17 | vsrc1 << 9 | ssrc0
    if family in VECTOR_FAMILIES and rng.random() < EXT_PROBABILITY and not (
        family == "VOP2" and opcode in VOP2_WITH_LITERAL
    ):
        # SDWA or DPP: the source code is the extension code, the next word is the extension word.
        code = rng.choice(EXT_CODES)
        return [(word & ~0x1FF) | code, extension_word(rng, code, (word >> 9) & 0xFF if family == "VOP2" else None)], None
    # The operands each family reads from the source fields (vsrc1 of VOP2 is a VGPR, never a literal).
    sources = {"SOP1": (ssrc0,), "VOP1": (ssrc0,), "VOP2": (ssrc0,), "SOP2": (ssrc0, ssrc1), "SOPC": (ssrc0, ssrc1), "VOPC": (ssrc0,)}
    needs_literal = LITERAL in sources.get(family, ()) or (family == "VOP2" and opcode in VOP2_WITH_LITERAL)
    return ([word, literal_bits(rng)] if needs_literal else [word]), None




# VOP3 names of the older encodings: the VOP1 and VOP2 operations written with VOP3 operands, the VOP3b
# carry forms, the 64-bit multiply-add and the lane operations. Their destinations are kept in range
# for the Python model (which indexes SGPRs and register pairs directly).
# Operations whose constant is a literal: the VOP3 form has no literal word unless a source is the literal code.
LITERAL_OPERATION_NAMES = ("v_madmk_f32", "v_madak_f32", "v_fmamk_f32", "v_fmaak_f32")
F16_TRANSCENDENTALS = frozenset({"v_rcp_f16", "v_sqrt_f16", "v_rsq_f16", "v_log_f16", "v_exp_f16", "v_sin_f16", "v_cos_f16"})
F16_ARITH_NAMES = tuple(name for name in F16_OPERATIONS if name not in F16_TRANSCENDENTALS)
# The 64-bit operations (including v_fma_f64 now that overflow returns ±infinity in both models).
WIDE_GENERIC = tuple(WIDE_OPERATIONS)
DIV_NAMES = (
    "v_div_scale_f32", "v_div_scale_f64", "v_div_fmas_f32", "v_div_fmas_f64", "v_div_fixup_f32", "v_div_fixup_f64",
)
VOP3_CARRY = ("v_add_co_u32", "v_sub_co_u32", "v_subrev_co_u32", "v_add_co_ci_u32", "v_sub_co_ci_u32",
              "v_subrev_co_ci_u32")
VOP3_MAD64 = ("v_mad_u64_u32", "v_mad_i64_i32")
VOP3_LANE = ("v_readfirstlane_b32", "v_readlane_b32", "v_writelane_b32", "v_mbcnt_lo_u32_b32", "v_mbcnt_hi_u32_b32")


def vop3_words(rng, opcode, name):
    """A VOP3 instruction: two words (the low half first), a third word when a source is the literal."""
    wide_sources = name in SAD_NAMES or name in VOP3_MAD64 or name.endswith(("_f64", "_u64", "_i64"))
    pool = list(range(256, 511)) if wide_sources else list(range(256, 512))
    if name in COMPARE64 and not name.endswith("class_f32"):
        pool = [code for code in OPERANDS if code != LITERAL] + list(range(256, 511))
    sources = [rng.choice(OPERANDS + pool) for _ in range(3)]
    # The SAD and f16 forms refuse unsupported modifiers: most of them get none, so the executed path is covered too.
    plain = (name in SAD_NAMES or name in MOVREL_VECTOR or name in F16_ARITH_NAMES) and rng.random() < 0.7
    if name in MOVREL_VECTOR:
        sources[0] = rng.randrange(256, 511)
    abs_bits = 0 if plain else rng.randrange(8)
    clamp = 0 if plain else rng.randrange(2)
    vdst = rng.randrange(256)
    if name in ("v_readfirstlane_b32", "v_readlane_b32"):
        vdst = rng.randrange(128)                                    # an SGPR destination
    if (name in VECTOR_COMPARE or name in COMPARE64) and "cmpx" not in name:
        vdst = rng.randrange(126)                                    # the compare writes the SGPR pair vdst
    field_8_14 = abs_bits
    if name in VOP3_CARRY or name in VOP3_MAD64:
        vdst = rng.randrange(255)
    if name.endswith("_f64") and name not in COMPARE64:
        vdst = rng.randrange(255)                                    # a 64-bit result takes vdst and vdst + 1
        field_8_14 = rng.randrange(126)                              # the carry SGPR pair sdst
    low = 0x35 << 26 | opcode << 16 | vdst | field_8_14 << 8 | (0 if plain else rng.randrange(16)) << 11 | clamp << 15
    high = sources[0] | sources[1] << 9 | sources[2] << 18 | (0 if plain else rng.randrange(4)) << 27 | (
        0 if plain else rng.randrange(8)) << 29
    words = [low, high]
    if LITERAL in sources:
        words.append(literal_bits(rng))
    return words, None


def vop3p_words(rng, opcode):
    """A VOP3P dot product: the default form most of the time (op_sel_hi all ones, no modifiers)."""
    sources = [rng.choice(OPERANDS + list(range(256, 512))) for _ in range(3)]
    plain = rng.random() < 0.8
    low = (0x33 << 26 | opcode << 16 | rng.randrange(256) | (1 << 14)
           | (0 if plain else rng.randrange(8) << 8 | rng.randrange(8) << 11 | rng.randrange(2) << 15))
    high = (sources[0] | sources[1] << 9 | sources[2] << 18
            | ((1 << 27 | 1 << 28) if plain else rng.randrange(4) << 27)
            | (0 if plain else rng.randrange(8) << 29))
    words = [low, high]
    if LITERAL in sources:
        words.append(literal_bits(rng))
    return words, None


def sop2_word(opcode, sdst, ssrc0, ssrc1):
    return 0b10 << 30 | opcode << 23 | sdst << 16 | ssrc1 << 8 | ssrc0


def test_random_scalar_programs_match_the_python_model():
    exe = binary()
    rng = random.Random(SEED)
    for _ in range(PROGRAMS):
        lanes = rng.choice((32, 64))
        scc = rng.random() < 0.5
        scalars = [(index, rng.getrandbits(32) if rng.random() < 0.5 else rng.randrange(8)) for index in range(16)]
        scalars.append((124, rng.randrange(256)))                    # M0, the index of the movrel forms
        words = program(*random_program(rng))
        assert_same(exe, lanes, scc, scalars, words)


def test_indexed_registers_past_32_bits_are_refused_by_both_models():
    """M0 is a full 32-bit value: a register number plus M0 past 2^32 is outside the model in both
    interpreters, never the register a wrapping 32-bit sum would give (47 + 0xFFFFFFFD is not s44)."""
    exe = binary()
    cases = [
        program(sop1("s_movrels_b32", 102, 47)), program(sop1("s_movrels_b64", 102, 47)),
        program(sop1("s_movreld_b32", 47, 44)), program(sop1("s_movreld_b64", 47, 44)),
        program(vop1("v_movrels_b32", 5, V0 + 3)), program(vop1("v_movreld_b32", 5, V0 + 3)),
        program(vop1("v_movrelsd_b32", 5, V0 + 3)),
    ]
    registers = [(44, 0x1234_5678), (45, 0x9ABC_DEF0), (47, 0x0BAD_F00D)]
    for words in cases:
        for m0 in (0xFFFF_FFFD, 0xFFFF_FFFF, 0x8000_0000):
            scalars = [(124, m0), *registers]
            assert run_python(32, False, scalars, words) == "error untranslated\n"
            assert_same(exe, 32, False, scalars, words)
        in_range = [(124, 1), *registers]
        assert run_python(32, False, in_range, words).startswith("steps")
        assert_same(exe, 32, False, in_range, words)


def test_vop3_literal_operations_match_the_python_model():
    """VOP3 v_madmk/madak/fmamk/fmaak_f32: refused without a literal, matching with a literal operand."""
    exe = binary()
    for name in LITERAL_OPERATION_NAMES:
        opcode = OPCODES["VOP3", name]
        low = 0x35 << 26 | opcode << 16 | 3
        without_literal = program(low, 256 | (257 << 9))
        assert run_python(32, False, [], without_literal) == "error untranslated\n"
        assert_same(exe, 32, False, [], without_literal)
        with_literal = program(low, LITERAL | (257 << 9), 0x4000_0000)
        assert run_python(32, False, [], with_literal).startswith("steps")
        assert_same(exe, 32, False, [], with_literal)


# Operand bit patterns the division helpers treat specially: zeros, infinities, NaNs, subnormals, the
# largest and smallest normal values, and values near the scale thresholds.
DIVISION_SPECIAL_WORDS = (
    0x0000_0000, 0x8000_0000, 0x3F80_0000, 0xBF80_0000, 0x7F80_0000, 0xFF80_0000, 0x7FC0_0000, 0x0000_0001,
    0x7F7F_FFFF, 0x0080_0000, 0x4000_0000, 0x0080_0001, 0x6000_0000, 0x1000_0000, 0x7480_0000,
)


def test_division_helpers_match_the_python_model():
    """One division helper per program, on SGPR operands (pairs for f64) of special and random bit patterns."""
    exe = binary()
    rng = random.Random(SEED + 1)
    for _ in range(200):
        name = rng.choice(DIV_NAMES)
        opcode = OPCODES["VOP3", name]
        f64 = name.endswith("_f64")
        sources = (0, 2, 4) if f64 else (0, 1, 2)
        plain = rng.random() < 0.5
        neg_bits = 0 if plain else rng.randrange(8)
        clamp = 0 if plain else rng.randrange(2)
        sdst = rng.randrange(126)
        low = 0x35 << 26 | opcode << 16 | sdst << 8 | clamp << 15
        high = sources[0] | sources[1] << 9 | sources[2] << 18 | (0 if plain else rng.randrange(4)) << 27 | neg_bits << 29
        scalars = [
            (index, rng.choice(DIVISION_SPECIAL_WORDS) if rng.random() < 0.6 else rng.getrandbits(32))
            for index in range(8)
        ] + [(106, rng.getrandbits(32))]
        assert_same(exe, 32, False, scalars, program(low, high))


CURATED_DOUBLE_BITS = (
    0x0000_0000_0000_0000, 0x8000_0000_0000_0000, 0x3FF0_0000_0000_0000, 0xC004_0000_0000_0000,
    0x3F5F_F5E4_8E13_A95E, 0x4E33_F8D6_9C53_9B7D, 0xD34B_EE65_6C88_EC6E, 0x7FF0_0000_0000_0000,
    0xFFF0_0000_0000_0000, 0x7FF8_0000_0000_0000, 0x7FF0_0000_0000_0001, 0x0000_0000_0000_0001,
    0x3FE0_0000_0000_0000, 0x3DDB_7CDF_D9D7_BDBB, 0x4159_9A5D_3A7F_D000,
)


def test_wide_operations_match_the_python_model():
    """64-bit operations (f64, 64-bit shifts, f64 fma included) on SGPR pairs of curated doubles."""
    exe = binary()
    rng = random.Random(SEED + 2)
    names = tuple(name for name in WIDE_OPERATIONS if ("VOP3", name) in OPCODES)
    for _ in range(200):
        name = rng.choice(names)
        opcode = OPCODES["VOP3", name]
        low = 0x35 << 26 | opcode << 16 | rng.randrange(1) | rng.randrange(8) << 8 | rng.randrange(2) << 15
        high = 0 | 2 << 9 | 4 << 18 | rng.randrange(4) << 27 | rng.randrange(8) << 29
        scalars = []
        for index in range(8):
            value = rng.choice(CURATED_DOUBLE_BITS) if rng.random() < 0.7 else rng.getrandbits(64)
            scalars += [(index, value & 0xFFFF_FFFF), (index + 8, value >> 32)]
        assert_same(exe, 32, False, scalars, program(low, high))


def test_fma_f64_overflow_and_signed_zero_match_the_python_model():
    """v_fma_f64 overflow (±infinity), product-overflow cancellation, and ±0.0 signs match Rust f64::mul_add."""
    exe = binary()
    opcode = OPCODES["VOP3", "v_fma_f64"]
    low = 0x35 << 26 | opcode << 16
    high = 0 | 2 << 9 | 4 << 18
    cases = [
        (0x7FEF_FFFF_FFFF_FFFF, 0x4000_0000_0000_0000, 0x0000_0000_0000_0000),  # +MAX * 2 + 0 -> +inf
        (0xFFEF_FFFF_FFFF_FFFF, 0x4000_0000_0000_0000, 0x0000_0000_0000_0000),  # -MAX * 2 + 0 -> -inf
        (0x7FEF_FFFF_FFFF_FFFF, 0x4000_0000_0000_0000, 0xFFEF_FFFF_FFFF_FFFF),  # +MAX * 2 - MAX -> +MAX
        (0x8000_0000_0000_0000, 0x3FF0_0000_0000_0000, 0x8000_0000_0000_0000),  # -0 * 1 + -0 -> -0
        (0x8000_0000_0000_0000, 0x3FF0_0000_0000_0000, 0x0000_0000_0000_0000),  # -0 * 1 + +0 -> +0
        (0xBFF0_0000_0000_0000, 0x3FF0_0000_0000_0000, 0x3FF0_0000_0000_0000),  # -1 * 1 + 1 -> +0
    ]
    for a, b, c in cases:
        scalars = [
            (0, a & 0xFFFF_FFFF), (1, a >> 32),
            (2, b & 0xFFFF_FFFF), (3, b >> 32),
            (4, c & 0xFFFF_FFFF), (5, c >> 32),
        ]
        assert_same(exe, 32, False, scalars, program(low, high))


def smem(name, sdata, sbase, offset=0, soffset=125, glc=0):
    """Les deux mots d'une instruction SMEM (sdata, sbase en SGPR, décalage signé 21 bits, soffset, glc)."""
    first = 0x3D << 26 | OPCODES["SMEM", name] << 18 | glc << 16 | sdata << 6 | sbase // 2
    return first, soffset << 25 | offset & 0x1FFFFF


def test_scalar_hints_match_the_python_model():
    words = program(sopk("s_version", 7, 0x2000),
                    *smem("s_atc_probe", 7, 4, offset=0x64),
                    *smem("s_atc_probe_buffer", 3, 8))
    for lanes in (32, 64):
        assert_same(binary(), lanes, True, [(3, 0x12345678), (7, 0xabcdef01)], words)


def test_scalar_memory_programs_match_the_python_model():
    exe = binary()
    cases = [
        # s_load : adresse 64 bits en SGPR, décalage immédiat ou par SOFFSET, décalage négatif (zéros)
        (32, False, [(0, 0x2000), (1, 0), (2, 12)], [(0x2000, [0, 0, 0xDEADBEEF, 0x11])],
         program(*smem("s_load_dword", 4, 0, offset=8), *smem("s_load_dwordx2", 6, 0, offset=8),
                 *smem("s_load_dword", 9, 0, soffset=2), *smem("s_load_dword", 10, 0, offset=-4))),
        # s_buffer_load : V# en s4..s6 (base, pas, nombre d'octets) ; hors du buffer, zéros
        (32, False, [(4, 0x3000), (5, 0), (6, 16)], [(0x3000, [1, 2, 3, 4])],
         program(*smem("s_buffer_load_dwordx2", 8, 4, offset=4), *smem("s_buffer_load_dwordx2", 10, 4, offset=12))),
        (64, False, [(4, 0x3000), (5, 0x10000), (6, 64)], [(0x3000, [5, 6, 7, 8, 9, 10, 11, 12])],
         program(*smem("s_buffer_load_dwordx4", 12, 4, offset=8), *smem("s_buffer_load_dwordx8", 16, 4))),
        # s_store puis s_load du même mot
        (32, False, [(0, 0x4000), (1, 0), (2, 0x1234), (3, 0xABCD)], [],
         program(*smem("s_store_dword", 2, 0, offset=4), *smem("s_load_dword", 9, 0, offset=4),
                 *smem("s_store_dwordx2", 2, 0, offset=16))),
        # atomiques SMEM : ancienne valeur rendue avec GLC, cmpswap, _x2 avec retenue, buffer
        (32, False, [(0, 0x5000), (1, 0), (3, 5)], [(0x5000, [7])],
         program(*smem("s_atomic_add", 3, 0, glc=1))),
        (32, False, [(0, 0x5000), (1, 0), (6, 9), (7, 7)], [(0x5000, [7])],
         program(*smem("s_atomic_cmpswap", 6, 0))),
        (32, False, [(0, 0x5008), (1, 0), (4, 1), (5, 0)], [(0x5008, [0xFFFFFFFF, 0])],
         program(*smem("s_atomic_add_x2", 4, 0, glc=1))),
        (32, False, [(4, 0x5000), (5, 0), (6, 16), (3, 0x77)], [(0x5000, [1, 2])],
         program(*smem("s_buffer_atomic_swap", 3, 4, offset=4, glc=1))),
        # cache control (sans effet), s_memtime (compteur d'instructions)
        (32, False, [(0, 0x6000)], [],
         program(*smem("s_dcache_inv", 0, 0), sop1("s_mov_b32", 1, 128), *smem("s_memtime", 10, 0))),
    ]
    for lanes, scc, scalars, memory, words in cases:
        assert run_python(lanes, scc, scalars, words, memory).startswith("steps"), "le cas doit s'exécuter"
        assert_same(exe, lanes, scc, scalars, words, memory)


def ds(name, vdst=0, addr=0, data0=0, data1=0, offset0=0, offset1=0):
    """Les deux mots d'une instruction DS (décalages, ADDR, DATA0, DATA1, VDST)."""
    first = 0x36 << 26 | OPCODES["DS", name] << 18 | offset1 << 8 | offset0
    return first, addr | data0 << 8 | data1 << 16 | vdst << 24


def words(*items):
    """Mots d'un programme : les tuples (instructions à deux mots) sont aplatis."""
    flat = []
    for item in items:
        flat.extend(item if isinstance(item, tuple) else [item])
    return flat


def test_local_data_share_programs_match_the_python_model():
    exe = binary()
    lane4 = vop2("v_lshlrev_b32", 4, 130, 0)     # v4 = lane * 4
    lane8 = vop2("v_lshlrev_b32", 5, 131, 0)     # v5 = lane * 8
    cases = [
        # mots 32 bits, puis lecture avec ou sans signe, 64 bits, sous-mots
        (32, words(lane4, vop1("v_mov_b32", 1, 255), 0xDEADBEEF,
                   ds("ds_write_b32", addr=4, data0=1), ds("ds_read_b32", vdst=6, addr=4))),
        (32, words(lane8, vop1("v_mov_b32", 1, 255), 0x0000FF80, vop1("v_mov_b32", 2, 255), 0x55AA55AA,
                   ds("ds_write_b64", addr=5, data0=1), ds("ds_read_b64", vdst=8, addr=5),
                   ds("ds_read_i8", vdst=10, addr=5), ds("ds_read_u8", vdst=11, addr=5),
                   ds("ds_read_i16", vdst=12, addr=5), ds("ds_read_u16", vdst=13, addr=5))),
        # read2 et write2 (offsets en octets), écriture de 96 bits et lecture de 128 bits
        (32, words(lane4, vop1("v_mov_b32", 1, 255), 0x01020304, vop1("v_mov_b32", 2, 255), 0xA0B0C0D0,
                   ds("ds_write2_b32", addr=4, data0=1, data1=2, offset0=0, offset1=1),
                   ds("ds_read2_b32", vdst=6, addr=4, offset0=0, offset1=1),
                   ds("ds_write_b96", addr=4, data0=1), ds("ds_read_b128", vdst=10, addr=4))),
        # formes st64 (décalages × 64 éléments), ds_nop, wave64
        (64, words(lane4, vop1("v_mov_b32", 1, 255), 0x77770001, vop1("v_mov_b32", 2, 255), 0x88880002,
                   ds("ds_write2st64_b32", addr=4, data0=1, data1=2, offset0=0, offset1=1),
                   ds("ds_read2st64_b32", vdst=6, addr=4, offset0=0, offset1=1), ds("ds_nop"))),
    ]
    for lanes, program_words in cases:
        python = run_python(lanes, False, [], program_words)
        assert python.startswith("steps"), "le cas doit s'exécuter"
        assert_same(exe, lanes, False, [], program_words)


def test_lds_atomic_programs_match_the_python_model():
    """Atomiques LDS : formes simples, étendues (_rtn ou non), wrxchg2 ; les résultats sont relus
    dans les VGPR (la LDS n'est pas dans l'état rendu)."""
    exe = binary()
    lane4 = vop2("v_lshlrev_b32", 4, 130, 0)     # v4 = lane * 4
    lane8 = vop2("v_lshlrev_b32", 5, 131, 0)     # v5 = lane * 8
    cases = [
        # formes simples 32 bits : add rtn, sub sans retour, max signé rtn, lecture finale
        (32, words(lane4, vop1("v_mov_b32", 1, 255), 0x00000010, vop1("v_mov_b32", 2, 255), 0xFFFFFFFF,
                   vop1("v_mov_b32", 3, 255), 0x00000005,
                   ds("ds_write_b32", addr=4, data0=1),
                   ds("ds_add_rtn_u32", vdst=6, addr=4, data0=3),
                   ds("ds_sub_u32", addr=4, data0=3),
                   ds("ds_max_rtn_i32", vdst=7, addr=4, data0=2),
                   ds("ds_read_b32", vdst=8, addr=4))),
        # formes étendues 64 bits : add, min non signé, rsub, lecture 64 bits
        (32, words(lane8, vop1("v_mov_b32", 1, 255), 0x00000007, vop1("v_mov_b32", 2, 255), 0x00000001,
                   ds("ds_write_b64", addr=5, data0=1),
                   vop1("v_mov_b32", 3, 255), 0x00000003, vop1("v_mov_b32", 4, 255), 0x00000000,
                   ds("ds_add_rtn_u64", vdst=10, addr=5, data0=3),
                   ds("ds_min_rtn_u64", vdst=12, addr=5, data0=1),
                   ds("ds_xor_rtn_b64", vdst=14, addr=5, data0=1),
                   ds("ds_read_b64", vdst=16, addr=5))),
        # mskor, cmpst (DATA0 compare, DATA1 écrit), wrap, rsub, wrxchg, b32 avec retour
        (32, words(lane4, vop1("v_mov_b32", 1, 255), 0xFFFF0000, vop1("v_mov_b32", 2, 255), 0xFF00FF00,
                   vop1("v_mov_b32", 3, 255), 0x000000AB, vop1("v_mov_b32", 5, 255), 0x00000004,
                   ds("ds_write_b32", addr=4, data0=1),
                   ds("ds_mskor_rtn_b32", vdst=6, addr=4, data0=2, data1=3),
                   ds("ds_cmpst_rtn_b32", vdst=7, addr=4, data0=3, data1=5),
                   ds("ds_wrap_rtn_b32", vdst=8, addr=4, data0=5, data1=3),
                   ds("ds_rsub_rtn_u32", vdst=9, addr=4, data0=2),
                   ds("ds_wrxchg_rtn_b32", vdst=10, addr=4, data0=3),
                   ds("ds_read_b32", vdst=11, addr=4))),
        # wrxchg2 : deux échanges à décalages de 1 élément, puis wave64 avec st64 en b64
        (32, words(lane4, vop1("v_mov_b32", 1, 255), 0x11111111, vop1("v_mov_b32", 2, 255), 0x22222222,
                   ds("ds_write2_b32", addr=4, data0=1, data1=2, offset0=0, offset1=1),
                   ds("ds_wrxchg2_rtn_b32", vdst=6, addr=4, data0=2, data1=1, offset0=0, offset1=1),
                   ds("ds_read2_b32", vdst=8, addr=4, offset0=0, offset1=1))),
        (64, words(lane8, vop1("v_mov_b32", 1, 255), 0x00000001, vop1("v_mov_b32", 2, 255), 0x00000002,
                   vop1("v_mov_b32", 20, 255), 0x00000003, vop1("v_mov_b32", 21, 255), 0x00000004,
                   ds("ds_wrxchg2st64_rtn_b64", vdst=10, addr=5, data0=1, data1=20, offset0=0, offset1=1),
                   ds("ds_read_b64", vdst=16, addr=5, offset1=2))),
    ]
    for lanes, program_words in cases:
        python = run_python(lanes, False, [], program_words)
        assert python.startswith("steps"), "le cas doit s'exécuter"
        assert_same(exe, lanes, False, [], program_words)


def test_lds_half_and_lane_exchange_programs_match_the_python_model():
    """Acces _d16 (moitie de VGPR gardee) et ds_swizzle/ds_permute/ds_bpermute, en wave32 et wave64."""
    exe = binary()
    lane4 = vop2("v_lshlrev_b32", 4, 130, 0)     # v4 = lane * 4
    cases = [
        # _d16 : lecture signee et non signee des moities basse et haute, ecritures _hi
        (32, words(lane4, vop1("v_mov_b32", 1, 255), 0x80FF7F80, vop1("v_mov_b32", 6, 255), 0xAAAAAAAA,
                   vop1("v_mov_b32", 7, 255), 0x55555555,
                   ds("ds_write_b32", addr=4, data0=1),
                   ds("ds_read_i8_d16", vdst=6, addr=4),
                   ds("ds_read_u8_d16_hi", vdst=6, addr=4, offset0=1),
                   ds("ds_read_u16_d16", vdst=7, addr=4),
                   ds("ds_read_u16_d16_hi", vdst=7, addr=4, offset0=2),
                   ds("ds_write_b8_d16_hi", addr=4, data0=1, offset0=8),
                   ds("ds_write_b16_d16_hi", addr=4, data0=1, offset0=12),
                   ds("ds_read_b32", vdst=8, addr=4, offset0=8))),
        # ds_swizzle : quad perm, reverse (BITMASK), puis permute et bpermute (decalage de 4 octets)
        (32, words(lane4,
                   ds("ds_swizzle_b32", vdst=6, data0=4, offset0=0xB1, offset1=0x80),
                   ds("ds_swizzle_b32", vdst=7, data0=4, offset0=0x1F, offset1=0x7C),
                   ds("ds_bpermute_b32", vdst=8, addr=4, data0=4, offset0=4),
                   ds("ds_permute_b32", vdst=9, addr=4, data0=4, offset0=4))),
        # wave64 : chaque moitie de 32 lignes echange seule
        (64, words(lane4,
                   ds("ds_swizzle_b32", vdst=6, data0=4, offset0=0xB1, offset1=0x80),
                   ds("ds_bpermute_b32", vdst=8, addr=4, data0=4, offset0=4),
                   ds("ds_permute_b32", vdst=9, addr=4, data0=4, offset0=4))),
    ]
    for lanes, program_words in cases:
        python = run_python(lanes, False, [], program_words)
        assert python.startswith("steps"), "le cas doit s'executer"
        assert_same(exe, lanes, False, [], program_words)


def vmem(family, name, vdst=0, addr=0, data=0, saddr=125, offset=0, glc=0):
    """Les deux mots d'une instruction FLAT, GLOBAL ou SCRATCH (décalage sur 12 bits, GLC, ADDR, DATA,
    SADDR, VDST)."""
    segment = {"FLAT": 0, "SCRATCH": 1, "GLOBAL": 2}[family]     # bits 14-15 du premier mot
    first = 0x37 << 26 | segment << 14 | OPCODES[family, name] << 18 | glc << 16 | offset & 0xFFF
    return first, addr | data << 8 | saddr << 16 | vdst << 24


def test_flat_global_scratch_programs_match_the_python_model():
    exe = binary()
    lane4 = vop2("v_lshlrev_b32", 4, 130, 0)     # v4 = lane * 4 ; v5 reste nul (adresse haute)
    cases = [
        # FLAT : chargements (dword, signé, court, paire), écriture, atomiques avec retour
        (32, [], [(0, list(range(64))), (0x200, [0x80FF7F80] * 64)], words(
            lane4,
            vmem("FLAT", "flat_load_dword", vdst=6, addr=4),
            vmem("FLAT", "flat_store_dword", addr=4, data=6, offset=256),
            vmem("FLAT", "flat_load_sbyte", vdst=8, addr=4, offset=0x200),
            vmem("FLAT", "flat_load_ushort", vdst=9, addr=4, offset=0x200),
            vmem("FLAT", "flat_load_dwordx2", vdst=10, addr=4, offset=0x200),
            vmem("FLAT", "flat_atomic_add", vdst=12, addr=4, data=6, offset=0x300, glc=1),
            vmem("FLAT", "flat_atomic_cmpswap", vdst=14, addr=4, data=6, offset=0x300, glc=1))),
        # GLOBAL avec SADDR (paire SGPR + VGPR 32 bits, décalage négatif), SCRATCH en adresse VGPR
        (64, [(2, 0x1000), (3, 0)], [(0x1000, list(range(64)))], words(
            lane4,
            vmem("GLOBAL", "global_load_dword", vdst=6, addr=4, saddr=2),
            vmem("GLOBAL", "global_store_dword", addr=4, data=6, saddr=2, offset=-4),
            vmem("GLOBAL", "global_load_ubyte", vdst=8, addr=4, saddr=2),
            vmem("SCRATCH", "scratch_store_dword", addr=4, data=6, offset=0x400),
            vmem("SCRATCH", "scratch_load_dword", vdst=9, addr=4, offset=0x400))),
    ]
    for lanes, scalars, memory, program_words in cases:
        python = run_python(lanes, False, scalars, program_words, memory)
        assert python.startswith("steps"), "le cas doit s'exécuter"
        assert_same(exe, lanes, False, scalars, program_words, memory)


def mubuf(name, vdata=0, vaddr=0, srsrc=0, soffset=125, offset=0, offen=0, idxen=0, glc=0):
    """Les deux mots d'une instruction MUBUF (décalage, OFFEN, IDXEN, GLC ; VADDR, VDATA, SRSRC en quadruplets
    de SGPR, SOFFSET)."""
    first = 0x38 << 26 | OPCODES["MUBUF", name] << 18 | glc << 14 | idxen << 13 | offen << 12 | offset & 0xFFF
    return first, vaddr | vdata << 8 | srsrc << 16 | soffset << 24


def test_buffer_programs_match_the_python_model():
    exe = binary()
    lane4 = vop2("v_lshlrev_b32", 4, 130, 0)     # v4 = lane * 4
    cases = [
        # descripteur à pas nul : adresse de base 0x2000, 256 octets ; accès hors des enregistrements
        (32, [(8, 0x2000), (9, 0), (10, 0x100)], [(0x2000, list(range(128)))], words(
            lane4,
            mubuf("buffer_load_dword", vdata=6, vaddr=4, srsrc=2, offen=1),
            mubuf("buffer_store_dword", vdata=6, vaddr=4, srsrc=2, offen=1, offset=0x80),
            mubuf("buffer_load_ubyte", vdata=8, vaddr=4, srsrc=2, offen=1, offset=0x10),
            mubuf("buffer_load_dword", vdata=10, vaddr=4, srsrc=2, idxen=1),
            mubuf("buffer_atomic_add", vdata=6, vaddr=4, srsrc=2, offen=1, offset=0x40, glc=1))),
        # pas de 16 octets avec indice : seuls les indices inférieurs à 8 existent (wave64)
        (64, [(8, 0x3000), (9, 16 << 16), (10, 8)], [(0x3000, list(range(64)))], words(
            lane4,
            mubuf("buffer_load_dword", vdata=6, vaddr=4, srsrc=2, idxen=1),
            mubuf("buffer_store_dword", vdata=6, vaddr=4, srsrc=2, idxen=1, offset=4),
            mubuf("buffer_load_sbyte", vdata=12, vaddr=4, srsrc=2, idxen=1, offen=0, glc=0))),
    ]
    for lanes, scalars, memory, program_words in cases:
        python = run_python(lanes, False, scalars, program_words, memory)
        assert python.startswith("steps"), "le cas doit s'exécuter"
        assert_same(exe, lanes, False, scalars, program_words, memory)


FORMAT_TABLE = REPO / "crates" / "anyxboxx-rdna2-ref" / "src" / "format_table.rs"
# Opcodes MTBUF « xyz » que les tests de LLVM n'essaient pas (rdna2.ISA_ONLY_MNEMONICS).
MTBUF_OPCODES = {**{name: opcode for (family, name), opcode in OPCODES.items() if family == "MTBUF"},
                 "tbuffer_load_format_xyz": 2, "tbuffer_store_format_xyz": 6}
IDENTITY = 4 | 5 << 3 | 6 << 6 | 7 << 9         # DST_SEL X, Y, Z, W
FORMAT_DATA = [0x807F01FF, 0x3C007E01, 0x0001FC00, 0x7FFF8000, 0xFFFFFFFF, 0x00000000, 0x81828384, 0x12345678,
               0x7C00FBFF, 0x00FF7F80, 0xC0000001, 0x40490FDB]


def test_the_generated_format_table_matches_the_python_one():
    from emu.rdna2_buffer_format import FORMATS
    from emu.rdna2_image import FORMAT_NAMES

    text = FORMAT_TABLE.read_text(encoding="utf-8")
    names = {int(code): name for code, name in re.findall(r'\((\d+), "(GFX10_FORMAT_[^"]*)"\)', text)}
    assert names == FORMAT_NAMES
    kinds = {"Unorm": 1, "Snorm": 2, "Uscaled": 3, "Sscaled": 4, "Uint": 5, "Sint": 6, "Float": 7}
    found = {
        int(code): (kinds[kind], tuple(int(width) for width in widths.split(", ")), int(size), int(align))
        for code, kind, widths, size, align in re.findall(
            r"code: (\d+),\s*kind: Kind::(\w+),\s*widths: &\[([\d, ]+)\],\s*size: (\d+),\s*align: (\d+)\b", text)
    }
    assert found == {code: (f.kind, f.widths, f.size, f.align) for code, f in FORMATS.items()}


def mtbuf(name, fmt, vdata=0, vaddr=0, srsrc=0, soffset=125, offset=0, offen=0, idxen=0, tfe=0):
    """Les deux mots d'une instruction MTBUF (MTBUFe_gfx10 de LLVM) : opcode bits 16-18 du premier mot et
    bit 21 du second (Inst{53} = op{3}), FORMAT bits 19-25, TFE bit 23 du second mot."""
    opcode = MTBUF_OPCODES[name]
    first = 0x3A << 26 | fmt << 19 | (opcode & 7) << 16 | idxen << 13 | offen << 12 | offset & 0xFFF
    return first, vaddr | vdata << 8 | srsrc << 16 | (opcode >> 3 & 1) << 21 | tfe << 23 | soffset << 24


def buffer_descriptor(srsrc, word3, base=0x2000, records=0x100, stride=0, word1_high=0):
    """Les quatre SGPR d'un V# (base 48 bits, pas, NUM_RECORDS, mot 3) au quadruplet `srsrc`."""
    first = srsrc * 4
    return [(first, base & 0xFFFFFFFF), (first + 1, (base >> 32) & 0xFFFF | stride << 16 | word1_high),
            (first + 2, records), (first + 3, word3)]


def format_loads(loads, offset=0):
    """Descripteurs et instructions pour une liste de (nom, mot 3 du V#, format MTBUF ou None)."""
    scalars, program_words = [], []
    for k, (name, word3, fmt) in enumerate(loads):
        srsrc, vdata = 2 + k, 8 + 4 * k
        scalars += buffer_descriptor(srsrc, word3)
        fields = dict(vdata=vdata, vaddr=4, srsrc=srsrc, offen=1, offset=offset)
        program_words += mubuf(name, **fields) if fmt is None else mtbuf(name, fmt, **fields)
    return scalars, program_words


def format_memory():
    rng = random.Random(SEED)
    return [(0x2000, FORMAT_DATA + [rng.getrandbits(32) for _ in range(64 - len(FORMAT_DATA))])]


MUBUF_LOADS = [
    ("buffer_load_format_xyzw", IDENTITY | 56 << 12, None),                      # 8_8_8_8_UNORM
    ("buffer_load_format_xyzw", 7 | 6 << 3 | 1 << 6 | 57 << 12, None),           # 8_8_8_8_SNORM, W Z 1 0
    ("buffer_load_format_xyzw", 4 | 5 << 3 | 1 << 6 | 23 << 12, None),           # 16_16_UNORM, X Y 1 0
    ("buffer_load_format_xy", IDENTITY | 24 << 12, None),                        # 16_16_SNORM
    ("buffer_load_format_xyzw", 1 | 4 << 3 | 1 << 6 | 7 << 9 | 60 << 12, None),  # 8_8_8_8_UINT, 1 X 1 W
    ("buffer_load_format_xyzw", IDENTITY | 61 << 12, None),                      # 8_8_8_8_SINT
    ("buffer_load_format_xyzw", IDENTITY | 58 << 12, None),                      # 8_8_8_8_USCALED
    ("buffer_load_format_xyzw", IDENTITY | 59 << 12, None),                      # 8_8_8_8_SSCALED
    ("buffer_load_format_xyzw", 4 | 5 << 3 | 1 << 6 | 29 << 12, None),           # 16_16_FLOAT, X Y 1 0
    ("buffer_load_format_xy", 4 | 1 << 3 | 22 << 12, None),                      # 32_FLOAT, X 1
    ("buffer_load_format_xyzw", IDENTITY | 50 << 12, None),                      # 2_10_10_10_UNORM
    ("buffer_load_format_xyzw", IDENTITY | 51 << 12, None),                      # 2_10_10_10_SNORM
    ("buffer_load_format_x", IDENTITY | 7 << 12, None),                          # 16_UNORM
    ("buffer_load_format_x", IDENTITY | 8 << 12, None),                          # 16_SNORM
    ("buffer_load_format_xy", 4 | 1 << 3 | 2 << 12, None),                       # 8_SNORM, X 1
    ("buffer_load_format_x", IDENTITY | 13 << 12, None),                         # 16_FLOAT
]
MTBUF_LOADS = [
    ("tbuffer_load_format_x", 0xFFF | 1 << 12, 1),      # 8_UNORM (DST_SEL du V# ignorés)
    ("tbuffer_load_format_xy", 1 << 12, 15),            # 8_8_SNORM
    ("tbuffer_load_format_xyz", 1 << 12, 74),           # 32_32_32_FLOAT
    ("tbuffer_load_format_xyzw", 1 << 12, 71),          # 16_16_16_16_FLOAT
    ("tbuffer_load_format_xyzw", 1 << 12, 75),          # 32_32_32_32_UINT
    ("tbuffer_load_format_x", 1 << 12, 12),             # 16_SINT
    ("tbuffer_load_format_xyzw", 0, 56),                # V# FORMAT INVALID : composantes nulles
    ("tbuffer_load_format_xyz", 1 << 12, 54),           # 2_10_10_10_UINT
    ("tbuffer_load_format_xy", 1 << 12, 26),            # 16_16_SSCALED
]


def test_typed_buffer_loads_match_the_python_model():
    exe = binary()
    lane4 = vop2("v_lshlrev_b32", 4, 130, 0)     # v4 = lane * 4
    cases = []
    for lanes in (32, 64):
        # décalage 0xF0 ou 0xE8 : les lignes hautes sortent des 256 octets (DST_SEL = 1 reste 1)
        for loads, offset in ((MUBUF_LOADS, 0), (MUBUF_LOADS, 0xF0), (MTBUF_LOADS, 0), (MTBUF_LOADS, 0xE8)):
            scalars, program_words = format_loads(loads, offset=offset)
            cases.append((lanes, scalars, words(lane4, *program_words)))
    # pas de 8 octets avec indice : seuls les indices inférieurs à 20 existent
    strided = buffer_descriptor(2, IDENTITY | 65 << 12, stride=8, records=20)
    cases.append((64, strided, words(mubuf("buffer_load_format_xyzw", vdata=8, vaddr=0, srsrc=2, idxen=1))))
    for lanes, scalars, program_words in cases:
        python = run_python(lanes, False, scalars, program_words, format_memory())
        assert python.startswith("steps"), f"le cas doit s'exécuter : {python}"
        assert_same(exe, lanes, False, scalars, program_words, format_memory())


STORES = [
    (mubuf, "buffer_store_format_xyzw", 3, IDENTITY | 61 << 12, None, 0x000),          # 8_8_8_8_SINT
    (mubuf, "buffer_store_format_xy", 4, 4 | 5 << 3 | 1 << 6 | 27 << 12, None, 0x080),  # 16_16_UINT
    (mubuf, "buffer_store_format_xyzw", 5, IDENTITY | 54 << 12, None, 0x100),          # 2_10_10_10_UINT
    (mubuf, "buffer_store_format_x", 6, IDENTITY | 22 << 12, None, 0x180),             # 32_FLOAT
    (mubuf, "buffer_store_format_xyz", 7, IDENTITY | 35 << 12, None, 0x200),           # 10_11_11_SINT
    (mtbuf, "tbuffer_store_format_xyzw", 8, 1 << 12, 70, 0x280),                       # 16_16_16_16_SINT
    (mtbuf, "tbuffer_store_format_xy", 9, 1 << 12, 62, 0x300),                         # 32_32_UINT
    (mtbuf, "tbuffer_store_format_x", 10, 0, 5, 0x380),                                # V# INVALID : rien
    (mtbuf, "tbuffer_store_format_xyz", 11, 1 << 12, 41, 0x3C0),                       # 11_11_10_UINT
]


def test_typed_buffer_stores_match_the_python_model():
    exe = binary()
    lane4 = vop2("v_lshlrev_b32", 4, 130, 0)
    for lanes in (32, 64):
        scalars = buffer_descriptor(2, IDENTITY | 20 << 12)
        program_words = [lane4, *mubuf("buffer_load_dwordx4", vdata=20, vaddr=4, srsrc=2, offen=1)]
        for encode, name, srsrc, word3, fmt, offset in STORES:
            scalars += buffer_descriptor(srsrc, word3, base=0x3000, records=0x400)
            fields = dict(vdata=20, vaddr=4, srsrc=srsrc, offen=1, offset=offset)
            program_words += encode(name, **fields) if fmt is None else encode(name, fmt, **fields)
        program_words = words(*program_words)
        python = run_python(lanes, False, scalars, program_words, format_memory())
        assert python.startswith("steps"), f"le cas doit s'exécuter : {python}"
        assert_same(exe, lanes, False, scalars, program_words, format_memory())


def refused_format_accesses():
    """(mot 3 du V#, bits hauts du mot 1, instruction) refusés par les deux modèles."""
    lds = mubuf("buffer_load_format_x", vdata=8, srsrc=2)
    return [
        (IDENTITY, 0, mubuf("buffer_load_format_x", vdata=8, srsrc=2)),                       # FORMAT INVALID
        (IDENTITY | 36 << 12, 0, mubuf("buffer_load_format_x", vdata=8, srsrc=2)),            # 10_11_11_FLOAT
        (IDENTITY | 1 << 12, 0, mubuf("buffer_store_format_x", vdata=8, srsrc=2)),            # 8_UNORM écrit
        (IDENTITY | 13 << 12, 0, mubuf("buffer_store_format_x", vdata=8, srsrc=2)),           # 16_FLOAT écrit
        (7 | 14 << 12, 0, mubuf("buffer_load_format_x", vdata=8, srsrc=2)),                   # DST_SEL W, 8_8
        (2 | 22 << 12, 0, mubuf("buffer_load_format_x", vdata=8, srsrc=2)),                   # DST_SEL 2
        (IDENTITY | 18 << 12, 0, mubuf("buffer_store_format_x", vdata=8, srsrc=2)),           # 1 composante sur 2
        (IDENTITY | 18 << 12, 0, mubuf("buffer_store_format_xy", vdata=8, srsrc=2)),          # DST_SEL_Z = Z
        (5 | 4 << 3 | 20 << 12, 0, mubuf("buffer_store_format_x", vdata=8, srsrc=2)),         # DST_SEL Y X
        (1 << 12, 0, mtbuf("tbuffer_load_format_xyz", 23, vdata=8, srsrc=2)),                 # 3 sur 16_16
        (1 << 12, 0, mtbuf("tbuffer_load_format_x", 22, vdata=8, srsrc=2, tfe=1)),            # tfe
        (IDENTITY | 27 << 12, 0, mubuf("buffer_load_format_x", vdata=8, srsrc=2, offset=1)),  # non aligné
        (IDENTITY | 22 << 12, 0, (lds[0] | 1 << 16, lds[1])),                                 # lds
        (IDENTITY | 22 << 12, 1 << 31, lds),                                                  # SWIZZLE_ENABLE
        (IDENTITY | 22 << 12 | 1 << 23, 0, lds),                                              # ADD_TID_ENABLE
    ]


def test_refused_typed_buffer_accesses_match_the_python_model():
    exe = binary()
    for word3, word1_high, instruction in refused_format_accesses():
        scalars = buffer_descriptor(2, word3, word1_high=word1_high)
        program_words = words(instruction)
        assert run_python(32, False, scalars, program_words) == "error untranslated\n", hex(word3)
        assert_same(exe, 32, False, scalars, program_words)


IMAGE_ADDRESS = 0x4000
COORDINATES = 0x2000                            # tableaux par ligne : décalages, z, s, t, lod, x, y, mip
SPECIAL_COORDINATES = (0.5, 0.0, 1.0, -0.25, float("nan"), float("inf"), float("-inf"), 1e30, -1e30,
                       0.999999, 3.75, -2.5, 0.125, 0.875)


def f32_bits(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


def mimg(name, vdata=0, vaddr=0, srsrc=4, ssamp=6, dmask=0xF, dim=1, unorm=0, nsa=(), first_flags=0,
         second_flags=0):
    """Mots d'une instruction MIMG gfx10 (MIMGe_gfx10 de LLVM) : opcode bits 18-24 et bit 0 (OPM) ; NSA bits
    1-2 suivis des mots d'adresses supplémentaires (un octet par VGPR)."""
    opcode = OPCODES["MIMG", name]
    extra = list(nsa) + [0] * (-len(nsa) % 4)
    first = (0x3C << 26 | (opcode & 0x7F) << 18 | opcode >> 7 | (len(extra) // 4) << 1 | dim << 3 | dmask << 8
             | unorm << 12 | first_flags)
    second = vaddr | vdata << 8 | srsrc << 16 | ssamp << 21 | second_flags
    packed = [int.from_bytes(bytes(extra[i:i + 4]), "little") for i in range(0, len(extra), 4)]
    return (first, second, *packed)


def lane_values(rng, lanes):
    """Les tableaux de coordonnées (un mot par ligne) lus par les programmes d'image."""
    def floats(special, low, high):
        values = [*special, *(rng.uniform(low, high) for _ in range(lanes))][:lanes]
        return [f32_bits(value) for value in values]

    # décalages _o petits (-3 à 3, pour rester près de l'image 4 x 4), un quart au hasard sur 16 bits
    offsets = [rng.getrandbits(16) if rng.random() < 0.25 else
               (rng.randrange(-3, 4) & 0x3F) | (rng.randrange(-3, 4) & 0x3F) << 8 for _ in range(lanes)]
    depth = floats((0.5, float("nan"), 0.0, 1.0), -0.2, 1.2)
    s = floats(SPECIAL_COORDINATES, -0.6, 1.6)
    t = floats(SPECIAL_COORDINATES[::-1], -0.6, 1.6)
    lod = floats((0.0, 1.0, -1.0, float("nan"), -0.0), -1.0, 1.0)
    x = [rng.randrange(6) for _ in range(lanes)]
    y = [rng.randrange(6) for _ in range(lanes)]
    mip = [int(rng.random() < 0.2) for _ in range(lanes)]
    return [offsets, depth, s, t, lod, x, y, mip]


def image_memory(rng, lanes, fmt):
    words_per_image = 16 * rdna2_image.TEXEL_BYTES[fmt] // 4 + 1
    if fmt in (rdna2_image.FORMAT_32_FLOAT, rdna2_image.FORMAT_32_32_FLOAT, rdna2_image.FORMAT_32_32_32_32_FLOAT):
        texels = [f32_bits(rng.uniform(-4.0, 4.0)) for _ in range(words_per_image)]
    else:
        texels = [rng.getrandbits(32) for _ in range(words_per_image)]
    coordinates = [value for array in lane_values(rng, lanes) for value in array + [0] * (64 - lanes)]
    return [(COORDINATES, coordinates), (IMAGE_ADDRESS, texels)]


SAMPLERS = [
    rdna2_image.encode_sampler(),                                                     # 6 : point, bord
    rdna2_image.encode_sampler(1, 1, min_filter=1),                                   # 7 : mirror, min linéaire
    rdna2_image.encode_sampler(0, 0, mag_filter=1, min_filter=1),                     # 8 : wrap, bilinéaire
    rdna2_image.encode_sampler(6, 6, mag_filter=1, min_filter=1, depth_compare=1),    # 9 : bordure, LESS
    rdna2_image.encode_sampler(7, 7, depth_compare=6, border_color=2),                # 10 : GEQUAL, blanc
    rdna2_image.encode_sampler(6, 7, border_color=1),                                 # 11 : noir opaque
    rdna2_image.encode_sampler(3, 3, unnormalized=1, mag_filter=1),                   # 12 : non normalisé
    rdna2_image.encode_sampler(2, 1, mag_filter=1, depth_compare=5),                  # 13 : NOTEQUAL
]


def image_program(fmt):
    """Charge les coordonnées par ligne, puis lit, échantillonne et écrit l'image 4 x 4 de format `fmt`."""
    lane4 = vop2("v_lshlrev_b32", 4, 130, 0)
    loads = [mubuf("buffer_load_dword", vdata=10 + k, vaddr=4, srsrc=2, offen=1, offset=k * 0x100) for k in range(8)]
    # v10 décalages, v11 z, v12 s, v13 t, v14 lod, v15 x, v16 y, v17 mip
    body = [
        mimg("image_load", vdata=20, vaddr=15),
        mimg("image_load_mip", vdata=24, vaddr=15, dmask=0b1010),
        mimg("image_get_resinfo", vdata=26, vaddr=17),
        mimg("image_sample", vdata=30, vaddr=12, ssamp=6),
        mimg("image_sample_l", vdata=34, vaddr=12, ssamp=7),
        mimg("image_sample_lz_o", vdata=38, vaddr=10, ssamp=8, dmask=0b0111, nsa=(12, 13)),
        mimg("image_sample_c_l_o", vdata=42, vaddr=10, ssamp=9),
        mimg("image_sample_c", vdata=46, vaddr=11, ssamp=10, dmask=0b1001),
        mimg("image_gather4_o", vdata=50, vaddr=10, ssamp=11, dmask=0b0100, nsa=(12, 13)),
        mimg("image_gather4_l", vdata=54, vaddr=12, ssamp=8, dmask=0b0001),
        mimg("image_sample", vdata=58, vaddr=12, ssamp=8, unorm=1),
        mimg("image_sample_o", vdata=62, vaddr=10, ssamp=12, nsa=(12, 13)),
        mimg("image_sample_c_lz", vdata=66, vaddr=11, ssamp=13),
        mimg("image_gather4", vdata=70, vaddr=12, ssamp=7, dmask=0b1000),
        mimg("image_store", vdata=30, vaddr=15, dmask=0xF),
        mimg("image_store_mip", vdata=34, vaddr=15, dmask=0b0101),
        mimg("image_store", vdata=58, vaddr=15, dmask=0b1110),
        mimg("image_load", vdata=74, vaddr=15),
    ]
    return words(lane4, *loads, *body)


def image_scalars(fmt, swizzle):
    scalars = buffer_descriptor(2, IDENTITY | 20 << 12, base=COORDINATES, records=0x800)
    scalars += list(enumerate(rdna2_image.encode_image(IMAGE_ADDRESS, fmt, 4, 4, swizzle), start=16))
    for k, sampler_words in enumerate(SAMPLERS):
        scalars += list(enumerate(sampler_words, start=(6 + k) * 4))
    return [(index, value) for index, value in scalars if value]


IMAGE_FORMATS = (
    (rdna2_image.FORMAT_8_UNORM, (4, 5, 6, 7)),
    (rdna2_image.FORMAT_8_8_UNORM, (5, 4, 1, 0)),
    (rdna2_image.FORMAT_8_8_8_8_UNORM, (4, 5, 6, 7)),
    (rdna2_image.FORMAT_8_8_8_8_UNORM, (7, 6, 5, 4)),
    (rdna2_image.FORMAT_32_FLOAT, (4, 4, 1, 0)),
    (rdna2_image.FORMAT_32_32_FLOAT, (4, 5, 6, 7)),
    (rdna2_image.FORMAT_32_32_32_32_FLOAT, (4, 5, 6, 7)),
    (rdna2_image.FORMAT_32_32_32_32_FLOAT, (6, 7, 2, 4)),
)


def test_image_programs_match_the_python_model():
    exe = binary()
    rng = random.Random(SEED)
    for fmt, swizzle in IMAGE_FORMATS:
        for lanes in (32, 64):
            memory = image_memory(rng, lanes, fmt)
            scalars, program_words = image_scalars(fmt, swizzle), image_program(fmt)
            python = run_python(lanes, False, scalars, program_words, memory)
            assert python.startswith("steps"), f"le cas doit s'exécuter : {python}"
            assert_same(exe, lanes, False, scalars, program_words, memory)


def refused_image_cases():
    """(T#, S#, instruction) que les deux modèles refusent."""
    good = rdna2_image.encode_image(IMAGE_ADDRESS, rdna2_image.FORMAT_32_FLOAT, 4, 4)
    sampler = rdna2_image.encode_sampler()
    sample = mimg("image_sample", vdata=8, vaddr=0)
    return [
        (good, sampler, mimg("image_sample_d", vdata=8)),                        # non pris en charge
        (good, sampler, mimg("image_atomic_add", vdata=8)),
        (good, sampler, mimg("image_sample", vdata=8, dim=2)),                   # 3D
        (good, sampler, mimg("image_sample", vdata=8, first_flags=1 << 16)),     # tfe
        (good, sampler, mimg("image_sample", vdata=8, first_flags=1 << 15)),     # r128
        (good, sampler, mimg("image_sample", vdata=8, first_flags=1 << 17)),     # lwe
        (good, sampler, mimg("image_sample", vdata=8, second_flags=1 << 30)),    # a16
        (good, sampler, mimg("image_load", vdata=8, second_flags=1 << 31)),      # d16
        (good, sampler, mimg("image_gather4", vdata=8, dmask=0b0011)),           # deux canaux
        (rdna2_image.encode_image(IMAGE_ADDRESS, 2, 4, 4), sampler, sample),     # 8_SNORM
        (rdna2_image.encode_image(IMAGE_ADDRESS, 22, 4, 4, last_level=1), sampler, sample),
        (good[:3] + [good[3] & ~(0xF << 28) | 8 << 28] + good[4:], sampler, sample),  # type 1D
        (good, rdna2_image.encode_sampler(4, 2), sample),                        # adressage 4
        (good, rdna2_image.encode_sampler(mag_filter=2), sample),                # anisotrope
        (good, rdna2_image.encode_sampler(border_color=3), sample),              # bordure du registre
    ]


def test_refused_image_instructions_match_the_python_model():
    exe = binary()
    for image_words, sampler_words, instruction in refused_image_cases():
        scalars = [(16 + i, value) for i, value in enumerate(image_words) if value]
        scalars += [(24 + i, value) for i, value in enumerate(sampler_words) if value]
        program_words = words(instruction)
        assert run_python(32, False, scalars, program_words) == "error untranslated\n", instruction
        assert_same(exe, 32, False, scalars, program_words)


D16_BIT = 1 << 31                               # mot 2 d'une instruction MIMG : d16


def refused_d16_accesses():
    """Formes _d16 typées (MUBUF, MTBUF) et images d16, à comparer avec le modèle Python."""
    buffer = buffer_descriptor(2, IDENTITY | 22 << 12)
    image = [(16 + i, value) for i, value in
             enumerate(rdna2_image.encode_image(IMAGE_ADDRESS, rdna2_image.FORMAT_32_FLOAT, 4, 4)) if value]
    image += [(24 + i, value) for i, value in enumerate(rdna2_image.encode_sampler()) if value]
    cases = []
    for (family, name), _ in sorted(OPCODES.items()):
        if "_format_d16" not in name:
            continue
        if family == "MUBUF":
            cases.append((buffer, mubuf(name, vdata=8, srsrc=2)))
        elif family == "MTBUF":
            cases.append((buffer, mtbuf(name, 22, vdata=8, srsrc=2)))
    for name in ("image_load", "image_load_mip", "image_store", "image_store_mip", "image_get_resinfo",
                 "image_sample", "image_sample_l", "image_sample_lz_o", "image_sample_c", "image_gather4"):
        cases.append((image, mimg(name, vdata=8, vaddr=0, dmask=1, second_flags=D16_BIT)))
    return cases


def test_d16_typed_and_image_accesses_match_the_python_model():
    exe = binary()
    cases = refused_d16_accesses()
    assert len(cases) >= 28, len(cases)
    for scalars, instruction in cases:
        program_words = words(instruction)
        for lanes in (32, 64):
            assert_same(exe, lanes, False, scalars, program_words)


def test_d16_buffer_formats_match_the_python_model():
    exe = binary()
    memory = [(0x2000, [0xFFEF_FFAF, 0x8009_7FF7, 0x7F80_0001, 0xFFFF_1234])]
    for family, encode in (("MUBUF", mubuf), ("MTBUF", mtbuf)):
        for (kind, name), _ in sorted(OPCODES.items()):
            if kind != family or "_format_d16" not in name:
                continue
            for fmt in range(1, 78):
                scalars = buffer_descriptor(2, IDENTITY | fmt << 12)
                instruction = (encode(name, fmt, vdata=8, srsrc=2) if family == "MTBUF"
                               else encode(name, vdata=8, srsrc=2))
                program_words = words(vop1("v_mov_b32", 8, LITERAL), 0xCAFE_BABE,
                                      vop1("v_mov_b32", 9, LITERAL), 0xFACE_1234, instruction)
                assert_same(exe, 32, False, scalars, program_words, memory)


FLOAT32_SPECIALS = (0x00000000, 0x80000000, 0x3F800000, 0xBF800000, 0x40000000, 0x3F000000, 0x7FC00001,
                    0x7F800001, 0xFFC00003, 0x7F800000, 0xFF800000, 0x7F7FFFFF, 0xFF7FFFFF, 0x00000001,
                    0x80000001, 0x007FFFFF, 0x3FC00000, 0x33800000)
FLOAT64_SPECIALS = (0x0000000000000000, 0x8000000000000000, 0x3FF0000000000000, 0xBFF0000000000000,
                    0x4000000000000000, 0x7FF8000000000001, 0x7FF0000000000001, 0xFFF8000000000003,
                    0x7FF0000000000000, 0xFFF0000000000000, 0x7FEFFFFFFFFFFFFF, 0x0000000000000001,
                    0x8000000000000001, 0x3CA0000000000000, 0x3FF8000000000000)
FLOAT_DATA32, FLOAT_DATA64, FLOAT_COMPARE32, FLOAT_COMPARE64 = 0x8000, 0x8400, 0x8800, 0x8C00
FLOAT_OLD, FLOAT_OLD_STEP = 0x10000, 0x400
FLOAT_VECTOR_ATOMICS = ("fcmpswap", "fmin", "fmax", "fcmpswap_x2", "fmin_x2", "fmax_x2")


def float_bits_values(rng, specials, width, lanes):
    return [rng.choice(specials) if rng.random() < 0.7 else rng.getrandbits(width) for _ in range(lanes)]


def compare_values(rng, old, width):
    """Valeurs comparées de fcmpswap/cmpst : souvent égales à l'ancienne (ou le zéro de signe opposé)."""
    sign = 1 << (width - 1)
    chosen = []
    for value in old:
        roll = rng.random()
        if roll < 0.4:
            chosen.append(value)
        elif roll < 0.6 and value & ~sign == 0:
            chosen.append(value ^ sign)
        else:
            chosen.append(rng.choice(FLOAT32_SPECIALS if width == 32 else FLOAT64_SPECIALS))
    return chosen


def split64(values):
    return [half for value in values for half in (value & 0xFFFFFFFF, value >> 32)]


def float_atomic_memory(rng, lanes, regions):
    """Données, valeurs comparées et anciennes valeurs (une copie par région) des atomiques flottants."""
    old32 = float_bits_values(rng, FLOAT32_SPECIALS, 32, lanes)
    old64 = float_bits_values(rng, FLOAT64_SPECIALS, 64, lanes)
    memory = [(FLOAT_DATA32, float_bits_values(rng, FLOAT32_SPECIALS, 32, lanes)),
              (FLOAT_DATA64, split64(float_bits_values(rng, FLOAT64_SPECIALS, 64, lanes))),
              (FLOAT_COMPARE32, compare_values(rng, old32, 32)),
              (FLOAT_COMPARE64, split64(compare_values(rng, old64, 64)))]
    for k, width in enumerate(regions):
        memory.append((FLOAT_OLD + k * FLOAT_OLD_STEP, old32 if width == 32 else split64(old64)))
    return memory


def float_operand_loads():
    """v4 = ligne × 4, v5 = ligne × 8 ; v10 données 32 bits, v11 comparées, v12-v13 et v14-v15 en 64 bits."""
    return [vop2("v_lshlrev_b32", 4, 130, 0), vop2("v_lshlrev_b32", 5, 131, 0),
            vmem("GLOBAL", "global_load_dword", vdst=10, addr=4, saddr=2),
            vmem("GLOBAL", "global_load_dword", vdst=11, addr=4, saddr=8),
            vmem("GLOBAL", "global_load_dwordx2", vdst=12, addr=5, saddr=6),
            vmem("GLOBAL", "global_load_dwordx2", vdst=14, addr=5, saddr=10)]


def float_atomic_scalars(regions):
    scalars = [(2, FLOAT_DATA32), (6, FLOAT_DATA64), (8, FLOAT_COMPARE32), (10, FLOAT_COMPARE64)]
    return scalars + [(12 + 2 * k, FLOAT_OLD + k * FLOAT_OLD_STEP) for k in range(regions)]


def float_vector_atomic_program():
    """GLOBAL (adresse SADDR + VGPR) puis FLAT (adresse VGPR 64 bits), chaque atomique sur sa région."""
    body, regions = [], []
    for family in ("GLOBAL", "FLAT"):
        for kind in FLOAT_VECTOR_ATOMICS:
            k, wide = len(regions), kind.endswith("_x2")
            regions.append(64 if wide else 32)
            data = (12 if wide else 10)
            name = f"{family.lower()}_atomic_{kind}"
            glc = int(k % 3 != 2)
            if family == "GLOBAL":
                body.append(vmem(family, name, vdst=20 + 2 * k, addr=5 if wide else 4, data=data, saddr=12 + 2 * k,
                                 glc=glc))
            else:
                # v6-v7 : adresse 64 bits de la région (v7 reste nul)
                body += [vop2("v_add_nc_u32", 6, LITERAL, 5 if wide else 4), FLOAT_OLD + k * FLOAT_OLD_STEP]
                body.append(vmem(family, name, vdst=20 + 2 * k, addr=6, data=data, glc=glc))
    return words(*float_operand_loads(), *body), regions


FLOAT_LDS_ATOMICS = ("ds_add_f32", "ds_add_rtn_f32", "ds_min_f32", "ds_min_rtn_f32", "ds_max_f32",
                     "ds_max_rtn_f32", "ds_cmpst_f32", "ds_cmpst_rtn_f32", "ds_min_f64", "ds_min_rtn_f64",
                     "ds_max_f64", "ds_max_rtn_f64", "ds_cmpst_f64", "ds_cmpst_rtn_f64")


def float_lds_atomic_program():
    """Anciennes valeurs écrites dans la LDS (une zone par atomique), l'atomique, puis la zone relue."""
    body = [vmem("GLOBAL", "global_load_dword", vdst=16, addr=4, saddr=12),
            vmem("GLOBAL", "global_load_dwordx2", vdst=18, addr=5, saddr=14)]
    for j, name in enumerate(FLOAT_LDS_ATOMICS):
        wide = name.endswith("f64")
        addr, old = (5, 18) if wide else (4, 16)
        offset = j * 0x200
        place = dict(offset0=offset & 0xFF, offset1=offset >> 8)
        data0, data1 = ((14 if wide else 11), (12 if wide else 10)) if "cmpst" in name else ((12 if wide else 10), 0)
        body.append(ds("ds_write_b64" if wide else "ds_write_b32", addr=addr, data0=old, **place))
        body.append(ds(name, vdst=30 + 2 * j, addr=addr, data0=data0, data1=data1, **place))
        body.append(ds("ds_read_b64" if wide else "ds_read_b32", vdst=60 + 2 * j, addr=addr, **place))
    return words(*float_operand_loads(), *body), [32, 64]


def test_float_atomic_programs_match_the_python_model():
    """Atomiques flottants GLOBAL, FLAT et LDS (fcmpswap, fmin, fmax, fadd, fcmpst, 32 et 64 bits) sur des
    valeurs spéciales (zéros signés, NaN calmes et signalants, infinis, dénormaux)."""
    exe = binary()
    rng = random.Random(SEED)
    for build in (float_vector_atomic_program, float_lds_atomic_program):
        program_words, regions = build()
        for lanes in (32, 64, 32, 64):
            memory = float_atomic_memory(rng, lanes, regions)
            scalars = float_atomic_scalars(len(regions))
            python = run_python(lanes, False, scalars, program_words, memory)
            assert python.startswith("steps"), f"le cas doit s'exécuter : {python}"
            assert_same(exe, lanes, False, scalars, program_words, memory)


def vop3p(name, vdst, sources, op_sel=0, op_sel_hi=0b111, neg_lo=0, neg_hi=0, clamp=0):
    low = (0x33 << 26 | OPCODES["VOP3P", name] << 16 | vdst | neg_hi << 8 | op_sel << 11
           | (op_sel_hi >> 2 & 1) << 14 | clamp << 15)
    high = sources[0] | sources[1] << 9 | sources[2] << 18 | (op_sel_hi & 3) << 27 | neg_lo << 29
    return low, high


def vop3p_cases():
    """(programme, exécuté) : les VOP3P hors produits scalaires et les produits scalaires avec op_sel,
    neg ou clamp, que le modèle Python refuse, et un produit scalaire sans modificateur."""
    sources = (V0 + 1, V0 + 2, V0 + 1)
    cases = [(vop3p(name, 9, sources), name in PACKED or name in PKF16_VOP3P)
             for (family, name) in sorted(OPCODES) if family == "VOP3P" and name not in DOT_OPERATIONS]
    cases += [(vop3p(name, 9, sources, **flags), runs)
              for name in PACKED for flags, runs in (({"op_sel": 5, "op_sel_hi": 2}, True),
                                                      ({"neg_lo": 1}, False),
                                                      ({"clamp": 1}, not any(x in name for x in ("mul", "shr", "shl"))))]
    # Packed f16 (VOP3P): every modifier is defined, the clamp included.
    cases += [(vop3p(name, 9, sources, **flags), True)
              for name in PACKED_F16 for flags in ({"op_sel": 5, "op_sel_hi": 2}, {"neg_lo": 1}, {"neg_hi": 6}, {"clamp": 1})]
    # Mixed precision and f16 dot product: op_sel[i] needs op_sel_hi[i]; op_sel[2] and neg_hi[2] are refused.
    cases += [(vop3p("v_fma_mix_f32", 9, sources, op_sel=2), True),
              (vop3p("v_fma_mixlo_f16", 9, sources, op_sel=1, op_sel_hi=0b110), False),
              (vop3p("v_fma_mixhi_f16", 9, sources, neg_lo=4, neg_hi=1, clamp=1), True),
              (vop3p("v_dot2_f32_f16", 9, sources, op_sel=4), False),
              (vop3p("v_dot2_f32_f16", 9, sources, neg_hi=4), False),
              (vop3p("v_dot2_f32_f16", 9, sources, neg_lo=4, clamp=1), True)]
    cases += [(vop3p("v_dot4_u32_u8", 9, sources, **flags), False)
              for flags in ({"op_sel": 1}, {"op_sel_hi": 0b011}, {"neg_lo": 2}, {"neg_hi": 4}, {"clamp": 1})]
    return cases + [(vop3p("v_dot2_i32_i16", 9, sources), True)]


def assert_select_cases(cases):
    exe = binary()
    setup = [vop2("v_mul_u32_u24", 1, LITERAL, 0), 0x00810F7F, vop1("v_cvt_f32_u32", 2, V0)]
    for instruction, runs in cases:
        program_words = program(*words(*setup, instruction))
        for lanes in (32, 64):
            python = run_python(lanes, False, [], program_words)
            assert python.startswith("steps") == runs, (instruction, python)
            assert_same(exe, lanes, False, [], program_words)


def test_vop3p_forms_match_the_python_model():
    assert_select_cases(vop3p_cases())


PKF16_SPECIAL_WORDS = (0x00000000, 0x80000000, 0x3C003C00, 0xBC00BC00, 0x7C007C00, 0xFC00FC00, 0x7E007E00,
                       0x7BFF7BFF, 0x00010001, 0x83FF83FF, 0x3F800000, 0xBF800000, 0x7F800000, 0xFF800000,
                       0x7FC00000, 0x7F800001, 0x00800000, 0x7F7FFFFF, 0x3C014000)
PKF16_VOP3P_NAMES = ("v_pk_fma_f16", "v_pk_add_f16", "v_pk_mul_f16", "v_pk_min_f16", "v_pk_max_f16",
                     "v_fma_mix_f32", "v_fma_mixlo_f16", "v_fma_mixhi_f16", "v_dot2_f32_f16")
PKF16_PROGRAMS = 200


def pkf16_operand(rng):
    """A 32-bit operand: halves and f32 values at the edges (NaN, infinities, subnormals, ties) or random bits."""
    return rng.choice(PKF16_SPECIAL_WORDS) if rng.random() < 0.4 else rng.getrandbits(32)


def pkf16_program(rng):
    """Words of one program: v1 to v3 loaded with operands, then a packed f16 form with random modifiers."""
    setup = []
    for index in (1, 2, 3):
        setup += [vop1("v_mov_b32", index, LITERAL), pkf16_operand(rng)]
    sources = (V0 + 1, V0 + 2, V0 + 3) if rng.random() < 0.8 else (V0 + 1, rng.choice((240, 242, 245, 248, 128)), V0 + 3)
    if rng.random() < 0.8:
        flags = {"op_sel": rng.randrange(8), "op_sel_hi": rng.randrange(8), "neg_lo": rng.randrange(8),
                 "neg_hi": rng.randrange(8), "clamp": rng.randrange(2)}
        if rng.random() < 0.5:
            flags = {"clamp": flags["clamp"]}
        instruction = vop3p(rng.choice(PKF16_VOP3P_NAMES), 9, sources, **flags)
    elif rng.random() < 0.5:
        instruction = vop3("v_pk_fmac_f16", 9, (V0 + 1, V0 + 2, 128), op_sel=rng.randrange(16),
                           clamp=rng.randrange(2), abs_bits=rng.randrange(8), neg=rng.randrange(8))
    else:
        instruction = vop2("v_pk_fmac_f16", 9, V0 + 1, 2)
    return words(*setup, instruction)


def test_pkf16_random_programs_match_the_python_model():
    exe = binary()
    rng = random.Random(SEED + 3)
    for _ in range(PKF16_PROGRAMS):
        program_words = program(*pkf16_program(rng))
        for lanes in (32, 64):
            assert_same(exe, lanes, False, [], program_words)


def test_pk_fmac_forms_match_the_python_model():
    assert_select_cases([(vop2("v_pk_fmac_f16", 9, V0 + 1, 2), True),
                         (vop3("v_pk_fmac_f16", 9, (V0 + 1, V0 + 2, 128), clamp=1), True),
                         (vop3("v_pk_fmac_f16", 9, (V0 + 1, V0 + 2, 128), op_sel=1), False),
                         (vop3("v_pk_fmac_f16", 9, (V0 + 1, V0 + 2, 128), neg=1), False)])


def vop3(name, vdst, sources, op_sel=0, clamp=0, abs_bits=0, neg=0, omod=0):
    low = 0x35 << 26 | OPCODES["VOP3", name] << 16 | vdst | abs_bits << 8 | op_sel << 11 | clamp << 15
    return low, sources[0] | sources[1] << 9 | sources[2] << 18 | omod << 27 | neg << 29


# Source registers of the register-group tests, filled with a different value in every lane and register,
# so that the programs that fit write non-zero, distinguishable results.
GROUP_SOURCES = (*range(1, 8), *range(250, 256))


def fill_group_sources():
    """v_mul_u32_u24 vN = (0x12345 + 0x1111 × N) × lane, for every register of GROUP_SOURCES."""
    words = []
    for register in GROUP_SOURCES:
        words += [vop2("v_mul_u32_u24", register, LITERAL, 0), 0x12345 + 0x1111 * register]
    return words


def test_a_register_group_past_v255_is_an_operand_error_in_rust():
    """A 64-bit result at v255 has no second register, a 64-bit source pair at v255 neither, and a 128-bit
    group has none from v253 up. The Rust returns an operand error before writing anything (it used to
    abort on an out-of-bounds index). The Python model raises IndexError, after writing the low half in
    the first active lane: that is a crash, not a refusal, and the parity harness turns it into no error
    line, so the two cannot be compared on these programs yet. Whether the Python should refuse them
    (and both print the same error line) is left to the maintainers; this test pins the current pairing.
    The same group one register lower fits, and both models run it to the same state."""
    exe = binary()
    wide, quad = (V0 + 2, V0 + 4, V0 + 6), (V0 + 2, V0 + 1, V0 + 4)
    cases = [                                                        # (name, sources, last register that fits)
        ("v_ashrrev_i64", (129, V0 + 2, 128), 254), ("v_add_f64", wide, 254), ("v_fma_f64", wide, 254),
        ("v_div_scale_f64", wide, 254), ("v_div_fmas_f64", wide, 254), ("v_div_fixup_f64", wide, 254),
        ("v_mad_u64_u32", (V0 + 1, V0 + 3, V0 + 2), 254), ("v_qsad_pk_u16_u8", quad, 254),
        ("v_mqsad_pk_u16_u8", quad, 254), ("v_mqsad_u32_u8", quad, 252),
    ]
    programs = [(program(*fill_group_sources(), *vop3(name, last + 1, sources)),
                 program(*fill_group_sources(), *vop3(name, last, sources)))
                for name, sources, last in cases]
    for name in ("v_floor_f64", "v_cvt_f64_i32"):                    # VOP1: a 64-bit result from one source
        programs.append((program(*fill_group_sources(), vop1(name, 255, V0 + 2)),
                         program(*fill_group_sources(), vop1(name, 254, V0 + 2))))
    # The wide S2 source of the SAD forms: a 64-bit pair at v255 (then v254), four VGPRs at v253 (then v252).
    for name, past, fits in (("v_qsad_pk_u16_u8", 255, 254), ("v_mqsad_pk_u16_u8", 255, 254),
                             ("v_mqsad_u32_u8", 253, 252)):
        programs.append((program(*fill_group_sources(), *vop3(name, 8, (V0 + 2, V0 + 1, V0 + past))),
                         program(*fill_group_sources(), *vop3(name, 8, (V0 + 2, V0 + 1, V0 + fits)))))
    for past, fits in programs:
        assert run_rust(exe, 32, False, [], past) == "error operand\n"
        with pytest.raises(IndexError):
            run_python(32, False, [], past)
        state = run_python(32, False, [], fits)
        assert state.startswith("steps")
        assert_same(exe, 32, False, [], fits)


def test_cndmask_mask_refusals_do_not_depend_on_exec_in_both_models():
    """VOP3 v_cndmask_b32 reads its lane mask (S2) as a scalar operand. A VGPR or a reserved code there is
    refused by both models whatever EXEC holds; a readable mask (VCC) runs in both, with or without lanes."""
    exe = binary()
    lanes_off = [(126, 0), (127, 0)]
    for lanes in (32, 64):
        for mask_code in (V0 + 4, 209):
            words = program(*vop3("v_cndmask_b32", 3, (V0 + 1, V0 + 2, mask_code)))
            for scalars in ([], lanes_off):
                assert run_python(lanes, False, scalars, words) == "error untranslated\n"
                assert_same(exe, lanes, False, scalars, words)
        words = program(vop1("v_mov_b32", 2, LITERAL), 0x5555, *vop3("v_cndmask_b32", 3, (V0 + 1, V0 + 2, 106)))
        for scalars in ([(106, 0xF0F0_F0F0), (107, 0x0F0F_0F0F)], [(106, 0xF0F0_F0F0), *lanes_off]):
            assert run_python(lanes, False, scalars, words).startswith("steps")
            assert_same(exe, lanes, False, scalars, words)


def test_vop3_integer_clamp_matches_the_python_model():
    exe = binary()
    names = ("v_add_nc_u32", "v_sub_nc_u32", "v_subrev_nc_u32", "v_add_co_u32",
             "v_sub_co_u32", "v_subrev_co_u32", "v_ashrrev_i32", "v_add3_u32", "v_mad_u64_u32")
    for lanes in (32, 64):
        for name in names:
            for a, b in ((0xFFFFFFFF, 2), (0, 2), (2, 0xFFFFFFFF)):
                setup = (vop1("v_mov_b32", 1, LITERAL), a, vop1("v_mov_b32", 2, LITERAL), b)
                instruction = vop3(name, 8, (V0 + 1, V0 + 2, 128), clamp=1, abs_bits=4)
                assert_same(exe, lanes, False, [], program(*words(*setup, instruction)))


def test_vop3_carry_mask_at_last_scalar_register():
    exe = binary()
    instruction = vop3("v_subrev_co_ci_u32", 8, (V0, 128, 128), abs_bits=7, op_sel=15)
    # SDST uses bits 8-14, including the op_sel bits: all ones designate s127.
    for lanes in (32, 64):
        program_words = program(*instruction)
        assert run_python(lanes, False, [], program_words).startswith("steps" if lanes == 32 else "error untranslated")
        assert_same(exe, lanes, False, [], program_words)


def operand_select_cases():
    """(instruction, exécutée) : v_sat_pk_u8_i16 (seul résultat 16 bits du modèle) avec op_sel ou clamp,
    refusé par le modèle Python, et les formes qu'il exécute en ignorant op_sel (résultat 32 bits)."""
    sources = (V0 + 1, V0 + 2, V0 + 1)
    cases = [(vop3("v_sat_pk_u8_i16", 9, sources, op_sel=op_sel, clamp=clamp), False)
             for op_sel, clamp in ((1, 0), (2, 0), (4, 0), (8, 0), (0, 1), (15, 1))]
    return cases + [(vop3("v_sat_pk_u8_i16", 9, sources), True),
                    (vop3("v_add_nc_u32", 9, sources, op_sel=15), True),
                    (vop3("v_fma_f32", 9, sources, op_sel=9), True),
                    (vop3("v_med3_i32", 9, sources, op_sel=6), True)]


def test_vop3_operand_select_forms_match_the_python_model():
    assert_select_cases(operand_select_cases())


COMPARE32_NAMES = sorted(name for (family, name) in OPCODES if family == "VOPC" and name in VECTOR_COMPARE)
COMPARE64_NAMES = sorted(name for (family, name) in OPCODES if family == "VOPC" and name in COMPARE64)
READABLE_SCALARS = (*range(0, 106), *range(128, 209), *range(240, 249))
COMPARE_PROGRAMS = 200


def vopc(name, src0, vsrc1):
    return 0x3E << 25 | OPCODES["VOPC", name] << 17 | vsrc1 << 9 | src0


def compare_operands():
    """v1-v6 : entiers et flottants différents par ligne (octets et demi-mots variés)."""
    return [vop2("v_mul_u32_u24", 1, LITERAL, 0), 0x01030507,
            vop2("v_xor_b32", 2, LITERAL, 1), 0x80FF7F00,
            vop1("v_cvt_f32_u32", 3, V0),
            vop2("v_sub_f32", 4, LITERAL, 3), 0x41000000,
            vop2("v_lshlrev_b32", 5, 144, 4),
            vop2("v_mul_f32", 6, LITERAL, 4), 0x7F7FFFFF]


def test_integer16_compares_match_the_python_model():
    exe = binary()
    names = [name for name in COMPARE32_NAMES if name.endswith(("_i16", "_u16"))]
    assert len(names) == 24
    setup = [vop2("v_mul_u32_u24", 1, LITERAL, 0), 0x80018001,
             vop2("v_xor_b32", 2, LITERAL, 1), 0xFFFF8000]
    for lanes in (32, 64):
        for name in names:
            for instruction in ((vopc(name, V0 + 1, 2),),
                                (vopc(name, 0xF9, 2), 1 | 5 << 16 | 4 << 24),
                                (vopc(name, 0xFA, 2), 0),
                                vop3(name, 20, (V0 + 1, V0 + 2, 128)),
                                vop3(name, 20, (V0 + 1, V0 + 2, 128), op_sel=1)):
                assert_same(exe, lanes, False, [], program(*words(*setup, instruction)))


def sdwa_compare_word(rng, lanes, vsrc1):
    """Mot SDWA d'une comparaison : sources lisibles (VGPR v0-v7 ou SGPR lisible), SDST hors de s127
    en wave64 (le modèle Python écrirait s128)."""
    ext = rng.getrandbits(32) & ~0xFF
    if ext >> 23 & 1:
        ext |= rng.choice(READABLE_SCALARS)
    else:
        ext |= rng.randrange(8)
    if ext >> 31 & 1 and vsrc1 not in READABLE_SCALARS:
        ext &= ~(1 << 31)
    if lanes == 64 and (ext >> 8 & 0x7F) == 127:
        ext &= ~(1 << 8)
    return ext


def compare_instruction(rng, lanes):
    """Une comparaison VOPC en SDWA (le plus souvent), DPP16 ou DPP8, ou une comparaison 64 bits ou class
    dont la source 0 est un code d'extension."""
    roll = rng.random()
    vsrc1 = rng.randrange(8) if rng.random() < 0.8 else rng.randrange(256)
    if roll < 0.75:
        return [vopc(rng.choice(COMPARE32_NAMES), 0xF9, vsrc1), sdwa_compare_word(rng, lanes, vsrc1)]
    if roll < 0.9:
        return [vopc(rng.choice(COMPARE32_NAMES), rng.choice((0xFA, 0xE9, 0xEA)), vsrc1), rng.getrandbits(32)]
    return [vopc(rng.choice(COMPARE64_NAMES), rng.choice(EXT_CODES), rng.randrange(7)), rng.getrandbits(32)]


def test_vopc_sdwa_and_dpp_programs_match_the_python_model():
    """VOPC en SDWA (sélections, signe, abs/neg, destination VCC, SDST ou EXEC), DPP refusé, et les
    comparaisons 64 bits dont la source 0 est un code d'extension (refus sur la première ligne active,
    masque nul quand EXEC est vide)."""
    exe = binary()
    rng = random.Random(SEED)
    outcomes = {"steps": 0, "error": 0}
    for _ in range(COMPARE_PROGRAMS):
        lanes = rng.choice((32, 64))
        scalars = [(index, rng.getrandbits(32)) for index in rng.sample(range(106), 12)]
        if rng.random() < 0.1:
            scalars += [(126, 0), (127, 0)]                 # EXEC vide : aucune ligne ne lit
        body = [word for _ in range(rng.randrange(1, 5)) for word in compare_instruction(rng, lanes)]
        program_words = program(*compare_operands(), *body)
        outcomes[run_python(lanes, False, scalars, program_words).split()[0]] += 1
        assert_same(exe, lanes, False, scalars, program_words)
    assert outcomes["steps"] >= COMPARE_PROGRAMS // 4, outcomes


def exp(target, enable, sources, compr=0, done=0, vm=0):
    """Instruction EXP (deux mots) : cible, bits EN, drapeaux, puis les quatre VGPR sources."""
    low = 0x3E << 26 | vm << 12 | done << 11 | compr << 10 | target << 4 | enable
    return [low, sources[0] | sources[1] << 8 | sources[2] << 16 | sources[3] << 24]


def export_operands():
    """v1-v4 : valeurs différentes par ligne (entier, flottant, motif)."""
    return [vop2("v_mul_u32_u24", 1, LITERAL, 0), 0x01030507,
            vop1("v_cvt_f32_u32", 2, V0),
            vop2("v_xor_b32", 3, LITERAL, 1), 0x80FF7F00,
            vop2("v_sub_f32", 4, LITERAL, 2), 0x41000000]


EXPORT_TARGETS = (0, 7, 8, 9, 12, 16, 17, 20, 21, 32, 39, 40, 63)   # mrt, mrtz, null, pos, prim, param, invalides
EXPORT_PROGRAMS = 120


def export_cases():
    """(lanes, scalaires, programme) : cibles, canaux, drapeaux done/vm, EXEC partiel ou vide, export
    répété sur une cible, et compr refusé (même EXEC vide)."""
    cases = []
    for lanes in (32, 64):
        exec_pairs = [[], [(126, 0x8000_0005), (127, 0x0000_0003)], [(126, 0), (127, 0)]]
        for scalars in exec_pairs:
            cases += [
                (lanes, scalars, program(*export_operands(), *exp(12, 0xF, (1, 2, 3, 4), done=1),
                                         *exp(32, 0b0101, (0, 9, 3, 200), vm=1), *exp(0, 0b1010, (4, 3, 2, 1)))),
                (lanes, scalars, program(*export_operands(), *exp(37, 0xF, (1, 1, 1, 1)),
                                         vop1("v_not_b32", 1, V0 + 1), *exp(37, 0b0011, (1, 2, 0, 0)))),
                (lanes, scalars, program(*exp(9, 0xF, (0, 0, 0, 0)), *exp(17, 0x0, (0, 0, 0, 0)),
                                         *exp(63, 0xF, (255, 0, 0, 0)))),
                (lanes, scalars, program(*export_operands(), *exp(1, 0xF, (1, 2, 3, 4), compr=1))),
            ]
    return cases


def test_export_programs_match_the_python_model():
    """EXP : enregistrements par cible et par ligne (canaux coupés à 0, dernier export gardé), toutes les
    cibles (null et invalides comprises), et le refus de compr ; plus des programmes tirés au hasard."""
    exe = binary()
    for lanes, scalars, words in export_cases():
        assert_same(exe, lanes, False, scalars, words)
    rng = random.Random(SEED)
    outcomes = {"steps": 0, "error": 0}
    for _ in range(EXPORT_PROGRAMS):
        lanes = rng.choice((32, 64))
        scalars = [(126, rng.getrandbits(32)), (127, rng.getrandbits(32))] if rng.random() < 0.5 else []
        body = []
        for _ in range(rng.randrange(1, 6)):
            sources = tuple(rng.randrange(8) if rng.random() < 0.9 else rng.randrange(256) for _ in range(4))
            body += exp(rng.choice(EXPORT_TARGETS), rng.getrandbits(4), sources, compr=int(rng.random() < 0.05),
                        done=rng.getrandbits(1), vm=rng.getrandbits(1))
            if rng.random() < 0.3:
                body.append(vop2("v_add_nc_u32", rng.randrange(1, 5), rng.choice((V0 + 1, V0 + 2, 129)), 3))
        program_words = program(*export_operands(), *body)
        outcomes[run_python(lanes, False, scalars, program_words).split()[0]] += 1
        assert_same(exe, lanes, False, scalars, program_words)
    assert outcomes["steps"] >= EXPORT_PROGRAMS // 2, outcomes


PARAMETERS = 0x54000                            # emu/rdna2_graphics.py : P0, P10, P20 par canal d'attribut
PRIMITIVE_WORDS = 8 * 4 * 3                     # param0-7, 4 canaux, 3 mots
INTERP_PRIMITIVES = 4
INTERP_PROGRAMS = 150
INTERP_SPECIALS = (0x7FC0_0000, 0xFFC0_0001, 0x7F80_0001, 0xFF80_0002, 0x7F80_0000, 0xFF80_0000,
                   0x8000_0000, 0x0000_0001, 0x7F7F_FFFF, 0x3F80_0000, 0xBF00_0000)


def vintrp(op, vdst, vsrc, attr, chan):
    """Instruction VINTRP (un mot) : op 0 p1, 1 p2, 2 mov, 3 sans nom."""
    return 0x32 << 26 | vdst << 18 | op << 16 | attr << 10 | chan << 8 | vsrc


def parameter_memory(rng, specials=0.1):
    """Zone PARAMETERS de INTERP_PRIMITIVES primitives : flottants ordinaires, parfois spéciaux."""
    values = []
    for _ in range(INTERP_PRIMITIVES * PRIMITIVE_WORDS):
        if rng.random() < specials:
            values.append(rng.choice(INTERP_SPECIALS))
        else:
            values.append(f32_bits(rng.uniform(-8.0, 8.0)))
    return [(PARAMETERS, values)]


def barycentrics():
    """v1 = i (ligne / 64), v2 = j (1 - i), comme les entrées v0/v1 d'un PS, dans d'autres registres."""
    return [vop1("v_cvt_f32_u32", 1, V0),
            vop2("v_mul_f32", 1, LITERAL, 1), 0x3C80_0000,
            vop2("v_sub_f32", 2, ONE_F, 1)]


def interp_cases():
    """(lanes, scalaires, programme, primitive, primitives) : p1 puis p2 puis export, mov et ses sources,
    primitives par ligne, EXEC partiel ou vide, attribut 8, opcode 3 et formes VOP3 refusés."""
    full = [vintrp(0, 3, 1, 2, 1), vintrp(1, 3, 2, 2, 1), vintrp(0, 4, 1, 7, 3), vintrp(1, 4, 2, 7, 3),
            vintrp(2, 5, 0, 0, 0), vintrp(2, 6, 1, 5, 2), vintrp(2, 7, 2, 6, 3), vintrp(2, 8, 9, 1, 1),
            *exp(32, 0xF, (3, 4, 5, 6))]
    cases = []
    for lanes in (32, 64):
        for scalars in ([], [(126, 0x0F0F_00F1), (127, 0x8000_0001)], [(126, 0), (127, 0)]):
            cases += [
                (lanes, scalars, program(*barycentrics(), *full), 0, None),
                (lanes, scalars, program(*barycentrics(), *full), 3, {0: 1, 5: 2, 33: 0, 63: 3}),
                (lanes, scalars, program(*barycentrics(), vintrp(0, 2, 2, 4, 0), vintrp(1, 2, 2, 4, 0)), 1, {}),
                (lanes, scalars, program(*barycentrics(), vintrp(0, 3, 1, 8, 0)), 0, None),
                (lanes, scalars, program(*barycentrics(), vintrp(3, 3, 1, 0, 0)), 0, None),
                (lanes, scalars, program(*vop3("v_interp_p1_f32", 3, (V0 + 1, 0, 0))), 0, None),
                (lanes, scalars, program(*vop3("v_interp_mov_f32", 3, (0, 0, 0))), 0, None),
                (lanes, scalars, program(*vop3("v_interp_p2_f16", 3, (V0 + 1, 0, V0 + 2))), 0, None),
            ]
    return cases


def interp_instruction(rng):
    roll = rng.random()
    if roll < 0.02:
        return [vintrp(3, rng.randrange(3, 10), rng.randrange(10), rng.randrange(8), rng.randrange(4))]
    attr = 8 + rng.randrange(56) if roll < 0.04 else rng.randrange(8)
    op = rng.randrange(3)
    vsrc = rng.randrange(4) if op == 2 and rng.random() < 0.8 else rng.randrange(10)
    word = [vintrp(op, rng.randrange(3, 10), vsrc, attr, rng.randrange(4))]
    if rng.random() < 0.15:
        word += exp(32 + rng.randrange(8), rng.getrandbits(4), tuple(rng.randrange(10) for _ in range(4)))
    return word


def test_interpolation_programs_match_the_python_model():
    """VINTRP : p1, p2 (produit puis somme arrondis séparément), mov, primitive commune ou par ligne,
    valeurs spéciales (NaN, infinis, sous-normaux), et les refus partagés ; plus des programmes au hasard."""
    exe = binary()
    rng = random.Random(SEED)
    for lanes, scalars, words, primitive, primitives in interp_cases():
        assert_same(exe, lanes, False, scalars, words, parameter_memory(rng), primitive=primitive,
                    primitives=primitives)
    outcomes = {"steps": 0, "error": 0}
    for _ in range(INTERP_PROGRAMS):
        lanes = rng.choice((32, 64))
        scalars = [(126, rng.getrandbits(32)), (127, rng.getrandbits(32))] if rng.random() < 0.4 else []
        primitive = rng.randrange(INTERP_PRIMITIVES)
        primitives = None
        if rng.random() < 0.5:
            primitives = {lane: rng.randrange(INTERP_PRIMITIVES) for lane in rng.sample(range(lanes), 12)}
        body = [word for _ in range(rng.randrange(1, 8)) for word in interp_instruction(rng)]
        program_words = program(*barycentrics(), *body)
        memory = parameter_memory(rng, specials=rng.choice((0.0, 0.1, 0.5)))
        graphics = {"primitive": primitive, "primitives": primitives}
        outcomes[run_python(lanes, False, scalars, program_words, memory, **graphics).split()[0]] += 1
        assert_same(exe, lanes, False, scalars, program_words, memory, **graphics)
    assert outcomes["steps"] >= INTERP_PROGRAMS // 2, outcomes


def program_counter_cases():
    """(scalaires, programme) : s_getpc_b64 (avec littéral, destinations NULL, EXEC, M0, impaire),
    sous-programme par s_swappc_b64 et par s_call_b64 avec retour par s_setpc_b64, s_setpc_b64 sur NULL
    (cible 0), et les refus : cible non alignée, hors du programme, poids fort non nul, paire impaire,
    opérande au-delà des SGPR, destination impaire après une cible valide."""
    end = sopp("s_endpgm")
    subroutine = [sop1("s_getpc_b64", 2, 0), sop2("s_add_u32", 2, 2, LITERAL), 20,
                  sop1("s_swappc_b64", 4, 2), sop1("s_mov_b32", 11, 133), end,
                  sop1("s_mov_b32", 10, 135), sop1("s_setpc_b64", 0, 4)]
    call = [sopk("s_call_b64", 6, 2), sop1("s_mov_b32", 11, 133), end,
            sop1("s_mov_b32", 10, 137), sop1("s_setpc_b64", 0, 6)]
    null_source = [sop2("s_add_u32", 20, 20, 129), sopc("s_cmp_eq_u32", 20, 130), sopp("s_cbranch_scc1", 1),
                   sop1("s_setpc_b64", 0, 125), end]
    return [
        ([], program(sop1("s_getpc_b64", 4, 0), sop1("s_getpc_b64", 8, LITERAL), 0xDEAD_BEEF)),
        ([], program(sop1("s_getpc_b64", 125, 0), sop1("s_getpc_b64", 124, 0), sop1("s_getpc_b64", 126, 0))),
        ([], program(sop1("s_getpc_b64", 5, 0))),
        ([], subroutine),
        ([], call),
        ([], null_source),
        ([(2, 6)], program(sop1("s_setpc_b64", 0, 2))),
        ([(2, 0x1000)], program(sop1("s_setpc_b64", 0, 2))),
        ([(2, 4), (3, 1)], program(sop1("s_setpc_b64", 0, 2))),
        ([(3, 4)], program(sop1("s_setpc_b64", 0, 3))),
        ([], program(sop1("s_setpc_b64", 0, 128))),
        ([(2, 4)], program(sop1("s_swappc_b64", 9, 2))),
        ([(2, 0x10)], program(sop1("s_swappc_b64", 8, 2))),
        ([], program(sopk("s_call_b64", 6, 0xFFF0))),
        ([], program(sopk("s_call_b64", 6, 0x0100))),
        ([], program(sopk("s_call_b64", 7, 0))),
    ]


def test_program_counter_programs_match_the_python_model():
    exe = binary()
    outcomes = {"steps": 0, "error": 0}
    for lanes in (32, 64):
        for scalars, words in program_counter_cases():
            outcomes[run_python(lanes, False, scalars, words).split()[0]] += 1
            assert_same(exe, lanes, False, scalars, words)
    assert outcomes == {"steps": 10, "error": 22}, outcomes


def test_buffer_cache_controls_match_the_python_model():
    """buffer_gl0_inv et buffer_gl1_inv : aucun effet, quels que soient leurs champs, entre deux accès."""
    exe = binary()
    rng = random.Random(SEED)
    scalars = [(4, 0x1000), (5, 0), (6, 0x100), (7, 0)]
    for lanes in (32, 64):
        for name in ("buffer_gl0_inv", "buffer_gl1_inv"):
            for _ in range(8):
                low, high = mubuf(name)
                noise = rng.getrandbits(64) & ~(0x3F << 26 | 0x7F << 18 | 1 << 25)
                words = program(*mubuf("buffer_store_dword", vdata=0, vaddr=0, srsrc=1, offen=1),
                                low | noise & 0xFFFF_FFFF, high | noise >> 32,
                                *mubuf("buffer_load_dword", vdata=3, vaddr=0, srsrc=1, offen=1))
                assert_same(exe, lanes, False, scalars, words)


def test_image_atomic_programs_match_the_python_model():
    """Parité Rust/Python pour les 16 image_atomic_* (32 et 64 bits, glc=0/1, OOB, NSA,
    contention, flottants NaN/±0, formats entiers 32/64 bits et refus de dmask/format)."""
    exe = binary()
    glc_bit = 1 << 13
    init32 = [0x0000_0000, 0x8000_0000, 0x7FC0_0000, 0xFFFF_FFFF,
              0xFFFF_FFFB, 5, 42, 0x3F80_0000] * 2
    init64 = [0xFFFF_FFFF, 0x0000_0001, 0x0000_0000, 0x8000_0000,
              0x0000_0000, 0x7FF8_0000, 0x3333_4444, 0x1111_2222] * 4
    mem32 = [(IMAGE_ADDRESS, init32)]
    mem64 = [(IMAGE_ADDRESS, init64)]

    # 1) Tous les 16 image_atomic_* en 32 bits (glc=0 et glc=1, wave32 et wave64, avec OOB + contention)
    for fmt in (rdna2_image.FORMAT_32_UINT, rdna2_image.FORMAT_32_SINT, rdna2_image.FORMAT_32_FLOAT):
        t_words = rdna2_image.encode_image(IMAGE_ADDRESS, fmt, 4, 4)
        scalars = [(16 + i, v) for i, v in enumerate(t_words) if v]
        insts = [
            vop2("v_and_b32", 1, 128 + 3, 0),            # x = lane & 3
            vop2("v_lshrrev_b32", 2, 128 + 2, 0),        # y = lane >> 2 (>= 4 est OOB)
        ]
        for idx, name in enumerate(sorted(rdna2_image.ATOMIC_OPS)):
            dmask = 0x3 if name in rdna2_image.ATOMIC_CMPSWAP_OPS else 0x1
            insts += [
                vop1("v_mov_b32", 4, LITERAL), 0xFFFF_FFFB if "s" in name else 0x4000_0000,
                vop1("v_mov_b32", 5, LITERAL), 0x0000_0000 if "f" in name else 42,
                *words(mimg(name, vdata=4, vaddr=1, dmask=dmask, first_flags=glc_bit if idx & 1 else 0)),
            ]
        # Contention multi-lignes + NSA + chevauchement vdata == vaddr
        insts += [
            vop1("v_mov_b32", 10, 129),
            vop1("v_mov_b32", 14, 129),
            *words(mimg("image_atomic_add", vdata=10, dmask=0x1, first_flags=glc_bit, nsa=(10, 14))),
        ]
        for lanes in (32, 64):
            assert_same(exe, lanes, False, scalars, program(*insts), mem32)

    # 2) Tous les 16 image_atomic_* en 64 bits (dmask=0x3 / 0xF) + load/store formats entiers 32/64 bits
    for fmt in (rdna2_image.FORMAT_32_32_UINT, rdna2_image.FORMAT_32_32_SINT, rdna2_image.FORMAT_32_32_FLOAT):
        t_words = rdna2_image.encode_image(IMAGE_ADDRESS, fmt, 4, 4)
        scalars = [(16 + i, v) for i, v in enumerate(t_words) if v]
        insts = [
            vop2("v_and_b32", 1, 128 + 3, 0),
            vop2("v_lshrrev_b32", 2, 128 + 2, 0),
            *words(mimg("image_load", vdata=8, vaddr=1, dmask=0xF)),
            *words(mimg("image_store", vdata=8, vaddr=1, dmask=0x3)),
        ]
        for idx, name in enumerate(sorted(rdna2_image.ATOMIC_OPS)):
            dmask = 0xF if name in rdna2_image.ATOMIC_CMPSWAP_OPS else 0x3
            insts += [
                vop1("v_mov_b32", 4, LITERAL), 0x0000_0002,
                vop1("v_mov_b32", 5, LITERAL), 0x4008_0000,
                vop1("v_mov_b32", 6, LITERAL), 0x0000_0000,
                vop1("v_mov_b32", 7, LITERAL), 0x0000_0000,
                *words(mimg(name, vdata=4, vaddr=1, dmask=dmask, first_flags=glc_bit if idx & 1 else 0)),
            ]
        assert_same(exe, 32, False, scalars, program(*insts), mem64)

    # 3) Refus partagés (dmask invalide et format incompatible avec la largeur)
    bad_cases = [
        (rdna2_image.FORMAT_32_UINT, mimg("image_atomic_add", vdata=4, vaddr=1, dmask=0x5)),
        (rdna2_image.FORMAT_32_UINT, mimg("image_atomic_cmpswap", vdata=4, vaddr=1, dmask=0x1)),
        (rdna2_image.FORMAT_8_8_8_8_UNORM, mimg("image_atomic_add", vdata=4, vaddr=1, dmask=0x1)),
        (rdna2_image.FORMAT_32_UINT, mimg("image_atomic_add", vdata=4, vaddr=1, dmask=0x3)),
        (rdna2_image.FORMAT_32_32_UINT, mimg("image_atomic_add", vdata=4, vaddr=1, dmask=0x1)),
    ]
    for fmt, inst in bad_cases:
        t_words = rdna2_image.encode_image(IMAGE_ADDRESS, fmt, 4, 4)
        scalars = [(16 + i, v) for i, v in enumerate(t_words) if v]
        pw = words(inst)
        assert run_python(32, False, scalars, pw) == "error untranslated\n"
        assert_same(exe, 32, False, scalars, pw)
