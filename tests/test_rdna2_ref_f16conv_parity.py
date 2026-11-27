"""Parity of f16 integer/normalized conversions, packs and min3/max3/med3 (`emu/rdna2_exec_f16conv.py`)."""
import random

from emu.rdna2_exec_f16conv import OPERATIONS as F16CONV_OPS
from tests.test_rdna2_ref_parity import (
    EXT_CODES,
    LITERAL,
    OPCODES,
    OPERANDS,
    SEED,
    V0,
    assert_same,
    binary,
    extension_word,
    program,
    run_python,
    vop1,
    vop2,
)

UNARY_NAMES = (
    "v_cvt_f16_u16",
    "v_cvt_f16_i16",
    "v_cvt_u16_f16",
    "v_cvt_i16_f16",
    "v_cvt_norm_i16_f16",
    "v_cvt_norm_u16_f16",
)
ALL_NAMES = tuple(sorted(F16CONV_OPS))
SPECIAL_HALVES = (
    0x0000, 0x8000, 0x0001, 0x8001, 0x03FF, 0x0400, 0x3555, 0x3800,
    0xB800, 0x3C00, 0xBC00, 0x4000, 0xC000, 0x7BFF, 0xFBFF, 0x7C00,
    0xFC00, 0x7C01, 0x7E00, 0xFFFF,
)
SPECIAL_WORDS = (
    0x0000_0000, 0x8000_0000, 0x3F00_0000, 0xBF00_0000, 0x3F80_0000,
    0xBF80_0000, 0x4000_0000, 0xC000_0000, 0x3800_0100, 0xB800_0100,
    0x7F80_0000, 0xFF80_0000, 0x7FC0_0001,
)


def vop3_conv(name, vdst, sources, *, abs_bits=0, op_sel=0, clamp=0, omod=0, neg_bits=0, literal=None):
    opcode = OPCODES["VOP3", name]
    s0, s1, s2 = (*sources, 128, 128)[:3]
    low = 0x35 << 26 | opcode << 16 | vdst | abs_bits << 8 | op_sel << 11 | clamp << 15
    high = s0 | s1 << 9 | s2 << 18 | omod << 27 | neg_bits << 29
    return [low, high] if literal is None else [low, high, literal]


def test_every_unary_f16conv_instruction_matches_the_python_model_on_all_65536_inputs():
    """All 65,536 16-bit inputs across the 6 unary integer and normalized f16 conversions."""
    exe = binary()
    for start in range(0, 0x10000, 64 * 32):
        words = []
        for step_idx in range(32):
            base = start + step_idx * 64
            words += [vop2("v_or_b32", 1, LITERAL, 0), base]
            for dst, name in enumerate(UNARY_NAMES, start=2):
                words.append(vop1(name, dst + (step_idx % 24) * 6, V0 + 1))
        assert_same(exe, 64, False, [], program(*words))


def test_all_twenty_f16conv_encodings_and_refusals_match_the_python_model():
    exe = binary()
    setup = [
        vop1("v_mov_b32", 1, LITERAL), 0xBC00_3800,   # lo = 0.5, hi = -1.0
        vop1("v_mov_b32", 2, LITERAL), 0x7E01_3C00,   # lo = 1.0, hi = sNaN
        vop1("v_mov_b32", 3, LITERAL), 0x8000_0000,   # lo = +0.0, hi = -0.0
        vop1("v_mov_b32", 4, LITERAL), 0x3F00_0000,   # 0.5f32
        vop1("v_mov_b32", 5, LITERAL), 0xBF40_0000,   # -0.75f32
        vop1("v_mov_b32", 6, LITERAL), 0xFFF0_002A,   # i16: +42, -16
    ]
    body = [
        *vop3_conv("v_cvt_f16_u16", 7, (248,), clamp=1),  # inline float on integer source (#226)
        *vop3_conv("v_cvt_f16_i16", 8, (V0 + 6,), clamp=1),
        *vop3_conv("v_cvt_u16_f16", 9, (V0 + 1,), abs_bits=1),
        *vop3_conv("v_cvt_i16_f16", 10, (V0 + 1,), neg_bits=1),
        *vop3_conv("v_cvt_norm_i16_f16", 11, (V0 + 1,), neg_bits=1),
        *vop3_conv("v_cvt_norm_u16_f16", 12, (V0 + 1,), abs_bits=1),
        *vop3_conv("v_cvt_pknorm_i16_f16", 13, (V0 + 1, V0 + 2), op_sel=0b0001, neg_bits=0b10),
        *vop3_conv("v_cvt_pknorm_u16_f16", 14, (V0 + 1, 240), abs_bits=0b01),
        *vop3_conv("v_cvt_pknorm_i16_f32", 15, (V0 + 4, V0 + 5), neg_bits=0b01),
        *vop3_conv("v_cvt_pknorm_u16_f32", 16, (V0 + 4, V0 + 5), abs_bits=0b10),
        *vop3_conv("v_pack_b32_f16", 17, (V0 + 1, V0 + 2), op_sel=0b0011, neg_bits=0b01),
        *vop3_conv("v_min3_f16", 18, (V0 + 1, V0 + 2, V0 + 3), op_sel=0b1101),
        *vop3_conv("v_max3_f16", 19, (V0 + 1, V0 + 2, V0 + 3), op_sel=0b1011, clamp=1),
        *vop3_conv("v_med3_f16", 20, (V0 + 3, V0 + 3, V0 + 1), op_sel=0b0001),
        *vop3_conv("v_med3_f16", 21, (V0 + 2, V0 + 1, 242), op_sel=0b1001, clamp=1),
    ]
    for lanes in (32, 64):
        prog = program(*setup, *body)
        assert run_python(lanes, False, [(126, 0xAAAA_5555)], prog).startswith("steps")
        assert_same(exe, lanes, False, [(126, 0xAAAA_5555)], prog)

    refusals = [
        program(vop1("v_cvt_u16_f16", 1, 0xF9), 0x0606_0600),
        program(*vop3_conv("v_min3_f16", 1, (V0 + 1, V0 + 2, V0 + 3), omod=1)),
        program(*vop3_conv("v_cvt_u16_f16", 1, (V0 + 1,), clamp=1)),
        program(*vop3_conv("v_pack_b32_f16", 1, (V0 + 1, V0 + 2), clamp=1)),
        program(*vop3_conv("v_cvt_f16_u16", 1, (V0 + 1,), op_sel=1)),
        program(*vop3_conv("v_cvt_f16_u16", 1, (V0 + 1,), abs_bits=1)),
        program(*vop3_conv("v_pack_b32_f16", 1, (V0 + 1, V0 + 2), op_sel=0b1000)),
        program(*vop3_conv("v_pack_b32_f16", 1, (V0 + 1, V0 + 2), op_sel=0b0100)),
        program(*vop3_conv("v_cvt_pknorm_i16_f32", 1, (V0 + 4, V0 + 5), op_sel=0b0001)),
        program(*vop3_conv("v_min3_f16", 1, (242, V0 + 1, V0 + 2), op_sel=0b0001)),
        program(*vop3_conv("v_min3_f16", 1, (LITERAL, V0 + 1, V0 + 2), op_sel=0b0001, literal=0x3C00_3C00)),
    ]
    for bad in refusals:
        assert run_python(32, False, [], bad) == "error untranslated\n"
        assert_same(exe, 32, False, [], bad)


def test_seeded_f16conv_programs_match_the_python_model():
    exe = binary()
    rng = random.Random(SEED ^ 0x2750)
    outcomes = {"steps": 0, "error": 0}
    for _ in range(250):
        lanes = rng.choice((32, 64))
        scalars = [(i, rng.choice(SPECIAL_HALVES) << 16 | rng.choice(SPECIAL_HALVES)) for i in range(6)]
        if rng.random() < 0.4:
            scalars += [(126, rng.getrandbits(32)), (127, rng.getrandbits(32))]
        body = []
        for reg in range(1, 6):
            val = rng.choice(SPECIAL_WORDS) if reg >= 4 else (
                rng.choice(SPECIAL_HALVES) << 16 | rng.choice(SPECIAL_HALVES)
            )
            body += [vop1("v_mov_b32", reg, LITERAL), val]
        for _ in range(rng.randrange(1, 6)):
            name = rng.choice(ALL_NAMES)
            kinds, _, result_kind = F16CONV_OPS[name]
            vdst = rng.randrange(1, 12)
            if name in UNARY_NAMES and rng.random() < 0.45:
                if rng.random() < 0.08:
                    ext = rng.choice(EXT_CODES)
                    body += [0x3F << 25 | vdst << 17 | OPCODES["VOP1", name] << 9 | ext, extension_word(rng, ext, None)]
                else:
                    src0 = rng.choice((*OPERANDS, *range(256, 262)))
                    body.append(vop1(name, vdst, src0))
                    if src0 == LITERAL:
                        body.append(rng.choice(SPECIAL_HALVES) << 16 | rng.choice(SPECIAL_HALVES))
                continue
            sources = [rng.choice((*OPERANDS, *range(256, 262))) for _ in range(3)]
            plain = rng.random() < 0.8
            op_sel = 0 if plain else rng.randrange(16)
            if plain and len(kinds) >= 2 and "w" not in kinds and all(s < 128 or s >= 256 for s in sources[:len(kinds)]):
                op_sel = rng.randrange(1 << len(kinds)) | ((rng.randrange(2) << 3) if len(kinds) == 3 else 0)
            clamp = (rng.randrange(2) if result_kind == "h" else 0) if plain else rng.randrange(2)
            abs_bits = (0 if "i" in kinds else rng.randrange(8)) if plain else rng.randrange(8)
            neg_bits = (0 if "i" in kinds else rng.randrange(8)) if plain else rng.randrange(8)
            omod = 0 if plain else rng.randrange(4)
            lit = None
            if LITERAL in sources:
                lit = rng.choice(SPECIAL_WORDS) if "w" in kinds else (
                    rng.choice(SPECIAL_HALVES) << 16 | rng.choice(SPECIAL_HALVES)
                )
            body += vop3_conv(
                name, vdst, sources, abs_bits=abs_bits, op_sel=op_sel,
                clamp=clamp, omod=omod, neg_bits=neg_bits, literal=lit,
            )
        words = program(*body)
        outcomes[run_python(lanes, False, scalars, words).split()[0]] += 1
        assert_same(exe, lanes, False, scalars, words)
    assert outcomes["steps"] >= 140 and outcomes["error"] >= 20, outcomes
