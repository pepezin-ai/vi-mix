"""Parity of scalar f16 transcendentals across all 65,536 binary16 inputs (`anyxboxx-rdna2-ref` vs Python)."""
import random

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

TRANSCENDENTALS = (
    "v_rcp_f16",
    "v_sqrt_f16",
    "v_rsq_f16",
    "v_log_f16",
    "v_exp_f16",
    "v_sin_f16",
    "v_cos_f16",
)
SPECIAL_HALVES = (
    0x0000, 0x8000, 0x0001, 0x8001, 0x03FF, 0x0400, 0x3118, 0x3400,
    0x3800, 0x3C00, 0xBC00, 0x4000, 0x4C00, 0xCC80, 0x7BFF, 0xFBFF,
    0x7C00, 0xFC00, 0x7C01, 0x7E00, 0xFFFF,
)


def vop3_trans(name, vdst, src0, *, abs_bits=0, op_sel=0, clamp=0, omod=0, neg_bits=0, literal=None):
    opcode = OPCODES["VOP3", name]
    low = 0x35 << 26 | opcode << 16 | vdst | abs_bits << 8 | op_sel << 11 | clamp << 15
    high = src0 | 128 << 9 | 128 << 18 | omod << 27 | neg_bits << 29
    return [low, high] if literal is None else [low, high, literal]


def test_every_f16_transcendental_matches_the_python_model_on_all_65536_inputs():
    """Bit-for-bit parity on every 16-bit input (0x0000..0xFFFF) for all 7 f16 transcendentals."""
    exe = binary()
    for start in range(0, 0x10000, 64 * 32):
        words = []
        for step_idx in range(32):
            base = start + step_idx * 64
            words += [vop2("v_or_b32", 1, LITERAL, 0), base]
            for dst, name in enumerate(TRANSCENDENTALS, start=2):
                words.append(vop1(name, dst + (step_idx % 24) * 7, V0 + 1))
        assert_same(exe, 64, False, [], program(*words))


def test_vop3_modifiers_and_refusals_for_f16_transcendentals_match_the_python_model():
    exe = binary()
    setup = [
        vop1("v_mov_b32", 1, LITERAL), 0xBC00_4000,  # lo = 2.0, hi = -1.0
        vop1("v_mov_b32", 2, LITERAL), 0xCAFE_BEEF,
    ]
    for lanes in (32, 64):
        body = list(setup)
        for idx, name in enumerate(TRANSCENDENTALS, start=3):
            body += [
                vop1("v_mov_b32", idx, V0 + 2),
                *vop3_trans(name, idx, V0 + 1, abs_bits=1, op_sel=0b1001, clamp=idx & 1),
                *vop3_trans(name, idx, 242, neg_bits=1),
            ]
        scalars = [(126, 0xF0F0_5A5A), (127, 0xA5A5_0F0F)]
        assert run_python(lanes, False, scalars, program(*body)).startswith("steps")
        assert_same(exe, lanes, False, scalars, program(*body))

    refusals = [
        program(vop1("v_rcp_f16", 1, 0xF9), 0x0606_0600),
        program(vop1("v_sin_f16", 1, 0xFA), 0),
        program(*vop3_trans("v_sqrt_f16", 1, V0 + 1, omod=1)),
        program(*vop3_trans("v_cos_f16", 1, 242, op_sel=1)),
        program(*vop3_trans("v_log_f16", 1, LITERAL, op_sel=1, literal=0x3C00_3C00)),
    ]
    for bad in refusals:
        assert run_python(32, False, [], bad) == "error untranslated\n"
        assert_same(exe, 32, False, [], bad)


def test_seeded_f16_transcendental_programs_match_the_python_model():
    exe = binary()
    rng = random.Random(SEED ^ 0x7A5C)
    outcomes = {"steps": 0, "error": 0}
    for _ in range(200):
        lanes = rng.choice((32, 64))
        scalars = [(i, rng.choice(SPECIAL_HALVES) << 16 | rng.choice(SPECIAL_HALVES)) for i in range(6)]
        if rng.random() < 0.4:
            scalars += [(126, rng.getrandbits(32)), (127, rng.getrandbits(32))]
        body = [
            vop1("v_mov_b32", reg, LITERAL) if step == 0 else val
            for reg in range(1, 5)
            for step, val in enumerate((0, rng.choice(SPECIAL_HALVES) << 16 | rng.choice(SPECIAL_HALVES)))
        ]
        for _ in range(rng.randrange(1, 6)):
            name = rng.choice(TRANSCENDENTALS)
            vdst = rng.randrange(1, 10)
            if rng.random() < 0.5:
                if rng.random() < 0.08:
                    ext = rng.choice(EXT_CODES)
                    body += [0x3F << 25 | vdst << 17 | OPCODES["VOP1", name] << 9 | ext, extension_word(rng, ext, None)]
                else:
                    src0 = rng.choice((*OPERANDS, *range(256, 262)))
                    body.append(vop1(name, vdst, src0))
                    if src0 == LITERAL:
                        body.append(rng.choice(SPECIAL_HALVES) << 16 | rng.choice(SPECIAL_HALVES))
            else:
                src0 = rng.choice((*OPERANDS, *range(256, 262)))
                plain = rng.random() < 0.8
                op_sel = (rng.choice((0, 0b0001, 0b1000, 0b1001)) if (src0 < 128 or src0 >= 256) else rng.choice((0, 0b1000))) if plain else rng.randrange(16)
                omod = 0 if plain else rng.randrange(4)
                lit = (rng.choice(SPECIAL_HALVES) << 16 | rng.choice(SPECIAL_HALVES)) if src0 == LITERAL else None
                body += vop3_trans(
                    name, vdst, src0, abs_bits=rng.randrange(8), op_sel=op_sel,
                    clamp=rng.randrange(2), omod=omod, neg_bits=rng.randrange(8), literal=lit,
                )
        words = program(*body)
        outcomes[run_python(lanes, False, scalars, words).split()[0]] += 1
        assert_same(exe, lanes, False, scalars, words)
    assert outcomes["steps"] >= 110 and outcomes["error"] >= 15, outcomes
