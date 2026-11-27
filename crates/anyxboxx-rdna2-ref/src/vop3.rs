//! VOP3 instructions of the model's own table (`VECTOR3` of `emu/rdna2_exec_vop3.py`): three sources,
//! input modifiers (abs, neg on the float ones) and output modifiers (clamp, omod on the float ones).
//!
//! Not in this module yet: the VOP3 encodings of VOP1, VOP2 and VOPC (opcodes below 0x200), the carry
//! and 64-bit multiply forms, lane operations, f64 and the divide, SAD and movrel helpers. They refuse
//! with `Untranslated`.

use crate::error::Error;
use crate::float::{self, bits_float, float_bits, py_max, py_min};
use crate::scalar::sign_extend;
use crate::wave::Wave;

const MASK32: u64 = 0xFFFF_FFFF;
const SIGN: u32 = 0x8000_0000;

/// The fields of a VOP3 word (64 bits: the first word is the low half).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fields {
    pub vdst: u32,
    pub abs: u32,
    /// VOP3b destination SGPR (carry and compare forms), bits 8 to 14 of the first word.
    pub sdst: u32,
    pub clamp: bool,
    pub sources: [u32; 3],
    pub omod: u32,
    pub neg: u32,
}

impl Fields {
    pub fn new(word: u64) -> Fields {
        Fields {
            vdst: (word & 0xFF) as u32,
            abs: ((word >> 8) & 0x7) as u32,
            sdst: ((word >> 8) & 0x7F) as u32,
            clamp: (word >> 15) & 1 == 1,
            sources: [
                ((word >> 32) & 0x1FF) as u32,
                ((word >> 41) & 0x1FF) as u32,
                ((word >> 50) & 0x1FF) as u32,
            ],
            omod: ((word >> 59) & 0x3) as u32,
            neg: ((word >> 61) & 0x7) as u32,
        }
    }
}

/// The VOP3 table of the model: names it executes here.
pub fn is_native(name: &str) -> bool {
    float_result_names(name).is_some() || integer_names(name)
}

/// Some(true) for the float results, Some(false) for the integer ones, None for other names.
fn float_result_names(name: &str) -> Option<bool> {
    match name {
        "v_fma_f32" | "v_mad_f32" | "v_fma_legacy_f32" | "v_min3_f32" | "v_max3_f32"
        | "v_med3_f32" | "v_ldexp_f32" | "v_cubeid_f32" | "v_cubesc_f32" | "v_cubetc_f32"
        | "v_cubema_f32" => Some(true),
        _ => None,
    }
}

fn integer_names(name: &str) -> bool {
    matches!(
        name,
        "v_mad_u32_u24"
            | "v_mad_i32_i24"
            | "v_bfe_u32"
            | "v_bfe_i32"
            | "v_bfi_b32"
            | "v_alignbit_b32"
            | "v_alignbyte_b32"
            | "v_min3_u32"
            | "v_max3_u32"
            | "v_med3_u32"
            | "v_min3_i32"
            | "v_max3_i32"
            | "v_med3_i32"
            | "v_lshl_add_u32"
            | "v_add_lshl_u32"
            | "v_add3_u32"
            | "v_lshl_or_b32"
            | "v_and_or_b32"
            | "v_or3_b32"
            | "v_xor3_b32"
            | "v_xad_u32"
            | "v_sad_u32"
            | "v_mul_lo_u32"
            | "v_mul_hi_u32"
            | "v_mul_hi_i32"
            | "v_bcnt_u32_b32"
            | "v_bfm_b32"
            | "v_perm_b32"
            | "v_mul_lo_i32"
            | "v_sad_u8"
            | "v_sad_hi_u8"
            | "v_lerp_u8"
    )
}

/// The Python `float_result`: the result is a float that the output modifiers apply to.
fn is_float_result(name: &str) -> bool {
    float_result_names(name).unwrap_or(false)
}

/// Median of three, as `sorted((a, b, c))[1]` computes it in CPython 3.12: the first run (reversed when
/// it descends), then a binary insertion of the last element. NaN compares false, as in Python.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // CPython tests `!(x < y)`, which NaN makes different from `>=`.
pub fn median<T: PartialOrd + Copy>(a: T, b: T, c: T) -> T {
    let mut v = [a, b, c];
    let mut run = 2;
    if v[1] < v[0] {
        while run < 3 && v[run] < v[run - 1] {
            run += 1;
        }
        v[..run].reverse();
    } else {
        while run < 3 && !(v[run] < v[run - 1]) {
            run += 1;
        }
    }
    for start in run..3 {
        let pivot = v[start];
        let (mut low, mut high) = (0, start);
        while low < high {
            let middle = low + (high - low) / 2;
            if pivot < v[middle] {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        v.copy_within(low..start, low + 1);
        v[low] = pivot;
    }
    v[1]
}

/// Python's `min(x, y, z)` and `max(x, y, z)`: the first operand unless a later one compares past it.
fn min3(x: f64, y: f64, z: f64) -> f64 {
    py_min(py_min(x, y), z)
}

fn max3(x: f64, y: f64, z: f64) -> f64 {
    py_max(py_max(x, y), z)
}

/// `_cube`: (face, sc, tc, 2 × major axis) of a direction, as the model computes the v_cube* results.
fn cube(x: f64, y: f64, z: f64) -> (f64, f64, f64, f64) {
    if z.abs() >= x.abs() && z.abs() >= y.abs() {
        (
            if z < 0.0 { 5.0 } else { 4.0 },
            if z < 0.0 { -x } else { x },
            -y,
            2.0 * z,
        )
    } else if y.abs() >= x.abs() {
        (
            if y < 0.0 { 3.0 } else { 2.0 },
            x,
            if y < 0.0 { -z } else { z },
            2.0 * y,
        )
    } else {
        (
            if x < 0.0 { 1.0 } else { 0.0 },
            if x < 0.0 { z } else { -z },
            -y,
            2.0 * x,
        )
    }
}

/// `_bitfield`: the field at `offset` of `width` bits (both masked to 5 bits), sign-extended when asked.
fn bitfield(value: u32, offset: u32, width: u32, signed: bool) -> u64 {
    let offset = offset & 31;
    let width = width & 31;
    if width == 0 {
        return 0;
    }
    let field = (u64::from(value) >> offset) & ((1u64 << width) - 1);
    if signed {
        sign_extend(field, width) as u64
    } else {
        field
    }
}

/// `_perm_b32`: the bytes of two sources selected by the bytes of a third (sign and zero bytes too).
fn perm(src0: u32, src1: u32, src2: u32) -> u32 {
    let mut out = 0u32;
    for shift in [0u32, 8, 16, 24] {
        let selector = (src2 >> shift) & 0xFF;
        let byte = match selector {
            0..=3 => (src1 >> (selector * 8)) & 0xFF,
            4..=7 => (src0 >> ((selector - 4) * 8)) & 0xFF,
            8 => {
                if src1 & 0x8000 != 0 {
                    0xFF
                } else {
                    0
                }
            }
            9 => {
                if src1 & 0x8000_0000 != 0 {
                    0xFF
                } else {
                    0
                }
            }
            10 => {
                if src0 & 0x8000 != 0 {
                    0xFF
                } else {
                    0
                }
            }
            11 => {
                if src0 & 0x8000_0000 != 0 {
                    0xFF
                } else {
                    0
                }
            }
            12 => 0,
            _ => 0xFF,
        };
        out |= byte << shift;
    }
    out
}

/// The four bytes of a value, least significant first.
fn bytes(value: u32) -> [u32; 4] {
    [
        value & 0xFF,
        value >> 8 & 0xFF,
        value >> 16 & 0xFF,
        value >> 24 & 0xFF,
    ]
}

fn sad_bytes(a: u32, b: u32) -> u64 {
    bytes(a)
        .iter()
        .zip(bytes(b))
        .map(|(x, y)| u64::from(x.abs_diff(y)))
        .sum()
}

fn lerp_bytes(a: u32, b: u32, c: u32) -> u64 {
    let (x, y, z) = (bytes(a), bytes(b), bytes(c));
    (0..4)
        .map(|i| u64::from((x[i] + y[i] + (z[i] & 1)) >> 1) << (8 * i))
        .sum()
}

/// The float of a value's bits, combined with the three operands as floats (`_float3`).
fn float3(a: u32, b: u32, c: u32, operation: impl Fn(f64, f64, f64) -> f64) -> u64 {
    u64::from(float_bits(operation(
        bits_float(a),
        bits_float(b),
        bits_float(c),
    )))
}

/// The value of one lane of a native VOP3 instruction, before output modifiers.
fn compute(name: &str, a: u32, b: u32, c: u32) -> Option<u64> {
    let (ai, bi, ci) = (u64::from(a), u64::from(b), u64::from(c));
    let (sa, sb, sc) = (
        sign_extend(ai, 32),
        sign_extend(bi, 32),
        sign_extend(ci, 32),
    );
    let fa = bits_float(a);
    let result = match name {
        "v_fma_f32" => float3(a, b, c, float::fma),
        "v_mad_f32" => float3(a, b, c, float::mul_add),
        "v_fma_legacy_f32" => float3(a, b, c, float::legacy_fma),
        "v_min3_f32" => float3(a, b, c, min3),
        "v_max3_f32" => float3(a, b, c, max3),
        "v_med3_f32" => float3(a, b, c, median),
        "v_ldexp_f32" => u64::from(float_bits(float::ldexp(fa, sign_extend(bi, 32)))),
        "v_cubeid_f32" => float3(a, b, c, |x, y, z| cube(x, y, z).0),
        "v_cubesc_f32" => float3(a, b, c, |x, y, z| cube(x, y, z).1),
        "v_cubetc_f32" => float3(a, b, c, |x, y, z| cube(x, y, z).2),
        "v_cubema_f32" => float3(a, b, c, |x, y, z| cube(x, y, z).3),
        "v_mad_u32_u24" => (ai & 0xFF_FFFF) * (bi & 0xFF_FFFF) + ci,
        "v_mad_i32_i24" => (sign_extend(ai, 24) * sign_extend(bi, 24) + sc) as u64,
        "v_bfe_u32" => bitfield(a, b, c, false),
        "v_bfe_i32" => bitfield(a, b, c, true),
        "v_bfi_b32" => (ai & bi) | (!ai & ci),
        "v_alignbit_b32" => (((ai << 32) | bi) >> (c & 31)) & MASK32,
        "v_alignbyte_b32" => (((ai << 32) | bi) >> ((c & 3) * 8)) & MASK32,
        "v_min3_u32" => ai.min(bi).min(ci),
        "v_max3_u32" => ai.max(bi).max(ci),
        "v_med3_u32" => median(ai, bi, ci),
        "v_min3_i32" => sa.min(sb).min(sc) as u64,
        "v_max3_i32" => sa.max(sb).max(sc) as u64,
        "v_med3_i32" => median(sa, sb, sc) as u64,
        "v_lshl_add_u32" => (ai << (b & 31)) + ci,
        "v_add_lshl_u32" => (ai + bi) << (c & 31),
        "v_add3_u32" => ai + bi + ci,
        "v_lshl_or_b32" => (ai << (b & 31)) | ci,
        "v_and_or_b32" => (ai & bi) | ci,
        "v_or3_b32" => ai | bi | ci,
        "v_xor3_b32" => ai ^ bi ^ ci,
        "v_xad_u32" => (ai ^ bi) + ci,
        "v_sad_u32" => u64::from(a.abs_diff(b)) + ci,
        "v_mul_lo_u32" | "v_mul_lo_i32" => ai * bi,
        "v_mul_hi_u32" => (ai * bi) >> 32,
        "v_mul_hi_i32" => ((sa * sb) >> 32) as u64,
        "v_bcnt_u32_b32" => u64::from(a.count_ones()) + bi,
        "v_bfm_b32" => ((1u64 << (a & 31)) - 1) << (b & 31),
        "v_perm_b32" => u64::from(perm(a, b, c)),
        "v_sad_u8" => sad_bytes(a, b) + ci,
        "v_sad_hi_u8" => (sad_bytes(a, b) << 16) + ci,
        "v_lerp_u8" => lerp_bytes(a, b, c),
        _ => {
            return None;
        }
    };
    Some(result)
}

/// The output of one lane: clamp and omod on the float results, a 32-bit mask otherwise.
fn output(fields: &Fields, value: u64, is_float: bool) -> u32 {
    if !is_float || !(fields.clamp || fields.omod != 0) {
        return (value & MASK32) as u32;
    }
    let scale = match fields.omod {
        1 => 2.0,
        2 => 4.0,
        3 => 0.5,
        _ => 1.0,
    };
    let mut number = bits_float(value as u32) * scale;
    if fields.clamp && !number.is_nan() {
        number = py_min(py_max(number, 0.0), 1.0);
    }
    float_bits(number)
}

/// Executes a native VOP3 instruction for every active lane. `Ok(false)` when `name` is not native.
pub fn execute(
    wave: &mut Wave,
    name: &str,
    word: u64,
    literal: Option<u32>,
) -> Result<bool, Error> {
    if name == "v_pk_fmac_f16" {
        crate::pkf16::execute_fmac_vop3(wave, word, literal)?;
        return Ok(true);
    }
    if crate::f16::supports(name) {
        crate::f16::execute_vop3(wave, name, word, literal)?;
        return Ok(true);
    }
    if crate::wide::execute(
        wave,
        name,
        &Fields::new(word).sources,
        Fields::new(word).vdst,
        literal,
        Fields::new(word).abs,
        Fields::new(word).neg,
        Fields::new(word).clamp,
        Fields::new(word).omod,
    )? {
        return Ok(true);
    }
    if crate::div::execute(wave, name, &Fields::new(word), literal)? {
        return Ok(true);
    }
    if crate::sad::operation(name).is_some() {
        return crate::sad::execute(wave, name, &Fields::new(word), literal);
    }
    let fields = Fields::new(word);
    let opcode = ((word >> 16) & 0x3FF) as u32;
    if (name.ends_with("_u16") || name.ends_with("_i16"))
        && crate::compare::is_compare(name)
        && (word >> 11) & 0xf != 0
    {
        return Err(Error::Untranslated(format!(
            "{name}: op_sel on a 16-bit compare"
        )));
    }
    if crate::vector::is_no_operation(name) {
        return Ok(true);
    }
    if crate::cmpf16::supports(name) {
        crate::cmpf16::execute_vop3(wave, name, word, literal)?;
        return Ok(true);
    }
    if crate::vop3_forms::lane(wave, name, &fields, literal)?
        || crate::vop3_forms::mad64(wave, name, &fields, literal)?
    {
        return Ok(true);
    }
    if crate::compare::execute_vop3(
        wave,
        name,
        [fields.sources[0], fields.sources[1]],
        literal,
        fields.vdst,
        fields.abs,
        fields.neg,
    )? {
        return Ok(true);
    }
    if opcode < 0x100 && crate::vop3_forms::compare(wave, name, &fields, literal)? {
        return Ok(true);
    }
    if crate::vop3_forms::carry(wave, name, &fields, literal)? {
        return Ok(true);
    }
    if name == "v_sat_pk_u8_i16" && ((word >> 11) & 0xF != 0 || fields.clamp) {
        return Err(Error::Untranslated(format!(
            "{name} avec op_sel/clamp (VOP3, résultat 16 bits)"
        )));
    }
    if crate::f16_trans::supports(name) {
        crate::f16_trans::execute_vop3(wave, name, word, literal)?;
        return Ok(true);
    }
    if crate::vop3_forms::operation(wave, name, opcode, &fields, literal)? {
        return Ok(true);
    }
    if crate::movrel::is_vector(name) {
        let fields = Fields::new(word);
        if fields.abs != 0 || fields.neg != 0 || fields.clamp || fields.omod != 0 {
            return Err(Error::Untranslated(format!(
                "{name} avec modificateurs VOP3"
            )));
        }
        return crate::movrel::vector(wave, name, fields.vdst, fields.sources[0], literal);
    }
    if crate::f16conv::supports(name) {
        crate::f16conv::execute_vop3(wave, name, word, literal)?;
        return Ok(true);
    }
    if !is_native(name) {
        return Ok(false);
    }
    let fields = Fields::new(word);
    let is_float = is_float_result(name);
    if fields.clamp && !is_float {
        return Err(Error::Untranslated(format!(
            "{name}: clamp on integer result"
        )));
    }
    let destination = fields.vdst as usize;
    for lane in wave.active()? {
        let mut values = [0u32; 3];
        for (index, code) in fields.sources.iter().enumerate() {
            let mut value = wave.read_lane(*code, lane, literal)?;
            if is_float {
                if fields.abs >> index & 1 == 1 {
                    value &= !SIGN;
                }
                if fields.neg >> index & 1 == 1 {
                    value ^= SIGN;
                }
            }
            values[index] = value;
        }
        let value = compute(name, values[0], values[1], values[2]).ok_or_else(|| {
            Error::Untranslated(format!("instruction non traduite : {name} (VOP3)"))
        })?;
        wave.v[destination][lane] = output(&fields, value, is_float);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_is_the_middle_of_the_sorted_three() {
        assert_eq!(median(3u32, 1, 2), 2);
        assert_eq!(median(1u32, 2, 3), 2);
        assert_eq!(median(2u32, 2, 1), 2);
    }

    #[test]
    fn bitfields_and_perm_pick_the_documented_bits_and_bytes() {
        assert_eq!(bitfield(0xABCD_1234, 4, 8, false), 0x23);
        assert_eq!(bitfield(0x80, 0, 8, true) as u32, 0xFFFF_FF80);
        assert_eq!(perm(0x0403_0201, 0x0807_0605, 0x0302_0100), 0x0807_0605);
    }

    #[test]
    fn ldexp_scales_by_powers_of_two_and_saturates() {
        assert_eq!(crate::float::ldexp(1.5, 3), 12.0);
        assert_eq!(crate::float::ldexp(1.0, 5000), f64::INFINITY);
        assert_eq!(crate::float::ldexp(-1.0, -5000), -0.0);
    }

    #[test]
    fn the_fields_of_a_word_are_read_at_their_documented_bits() {
        let word = 1u64 | 1 << 15 | 3 << 59 | 0x1FF << 32 | 0x100 << 41;
        let fields = Fields::new(word);
        assert_eq!(fields.vdst, 1);
        assert!(fields.clamp);
        assert_eq!(
            (fields.omod, fields.sources[0], fields.sources[1]),
            (3, 0x1FF, 0x100)
        );
    }
}
