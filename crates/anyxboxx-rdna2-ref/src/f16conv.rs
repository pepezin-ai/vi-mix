//! Half-precision integer and normalized conversions, packs and three-operand min/max/med3 (`VOP1` and
//! `VOP3`), ported from `emu/rdna2_exec_f16conv.py` (20 encodings).

use crate::error::Error;
use crate::float::bits_float;
use crate::half::{
    HALF_INFINITY, HALF_MASK, HALF_SIGN, half_clamp, half_from_f64, half_max, half_min,
    half_to_f64, is_nan, read_half,
};
use crate::vop3::Fields;
use crate::wave::{VGPR_FIRST, Wave};

const EXTENSION_CODES: [u32; 4] = [0xE9, 0xEA, 0xF9, 0xFA];
const FLOAT_SIGN: u32 = 0x8000_0000;
const INT16_SCALE: f64 = 32767.0;
const UINT16_SCALE: f64 = 65535.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Src {
    Half,
    Int16,
    Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dst {
    Half,
    Int16,
    Word,
}

type Op = fn(u32, u32, u32) -> u32;

const I: &[Src] = &[Src::Int16];
const H: &[Src] = &[Src::Half];
const HH: &[Src] = &[Src::Half, Src::Half];
const WW: &[Src] = &[Src::Word, Src::Word];
const HHH: &[Src] = &[Src::Half, Src::Half, Src::Half];

fn saturated(bits: u32, low: i32, high: i32) -> u32 {
    if is_nan(bits) {
        return 0;
    }
    if bits & 0x7FFF == HALF_INFINITY {
        let bound = if bits & HALF_SIGN != 0 { low } else { high };
        return (bound as u32) & HALF_MASK;
    }
    let truncated = half_to_f64(bits).trunc() as i32;
    (truncated.clamp(low, high) as u32) & HALF_MASK
}

/// Exact product `clamp(value) * scale` rounded to nearest even (40-bit exact in f64 for any f16 or f32).
fn normalized(value: f64, scale: f64, signed: bool) -> u32 {
    if value.is_nan() {
        return 0;
    }
    let low = if signed { -1.0 } else { 0.0 };
    let clamped = value.clamp(low, 1.0);
    ((clamped * scale).round_ties_even() as i32 as u32) & HALF_MASK
}

fn pknorm_half(a: u32, b: u32, scale: f64, signed: bool) -> u32 {
    normalized(half_to_f64(b), scale, signed) << 16 | normalized(half_to_f64(a), scale, signed)
}

fn pknorm_word(a: u32, b: u32, scale: f64, signed: bool) -> u32 {
    normalized(bits_float(b), scale, signed) << 16 | normalized(bits_float(a), scale, signed)
}

fn min3(a: u32, b: u32, c: u32) -> u32 {
    half_min(half_min(a, b), c)
}

fn max3(a: u32, b: u32, c: u32) -> u32 {
    half_max(half_max(a, b), c)
}

fn med3(a: u32, b: u32, c: u32) -> u32 {
    if is_nan(a) || is_nan(b) || is_nan(c) {
        return min3(a, b, c);
    }
    let same = |x: u32, y: u32| x == y || ((x | y) & 0x7FFF) == 0;
    let top = max3(a, b, c);
    if same(top, a) {
        half_max(b, c)
    } else if same(top, b) {
        half_max(a, c)
    } else {
        half_max(a, b)
    }
}

fn spec(name: &str) -> Option<(&'static [Src], Op, Dst)> {
    Some(match name {
        "v_cvt_f16_u16" => (I, |a, _, _| half_from_f64(f64::from(a)), Dst::Half),
        "v_cvt_f16_i16" => (
            I,
            |a, _, _| half_from_f64(f64::from(a as u16 as i16)),
            Dst::Half,
        ),
        "v_cvt_u16_f16" => (H, |a, _, _| saturated(a, 0, 0xFFFF), Dst::Int16),
        "v_cvt_i16_f16" => (H, |a, _, _| saturated(a, -0x8000, 0x7FFF), Dst::Int16),
        "v_cvt_norm_i16_f16" => (
            H,
            |a, _, _| normalized(half_to_f64(a), INT16_SCALE, true),
            Dst::Int16,
        ),
        "v_cvt_norm_u16_f16" => (
            H,
            |a, _, _| normalized(half_to_f64(a), UINT16_SCALE, false),
            Dst::Int16,
        ),
        "v_cvt_pknorm_i16_f16" => (
            HH,
            |a, b, _| pknorm_half(a, b, INT16_SCALE, true),
            Dst::Word,
        ),
        "v_cvt_pknorm_u16_f16" => (
            HH,
            |a, b, _| pknorm_half(a, b, UINT16_SCALE, false),
            Dst::Word,
        ),
        "v_cvt_pknorm_i16_f32" => (
            WW,
            |a, b, _| pknorm_word(a, b, INT16_SCALE, true),
            Dst::Word,
        ),
        "v_cvt_pknorm_u16_f32" => (
            WW,
            |a, b, _| pknorm_word(a, b, UINT16_SCALE, false),
            Dst::Word,
        ),
        "v_pack_b32_f16" => (HH, |a, b, _| (b << 16) | a, Dst::Word),
        "v_min3_f16" => (HHH, min3, Dst::Half),
        "v_max3_f16" => (HHH, max3, Dst::Half),
        "v_med3_f16" => (HHH, med3, Dst::Half),
        _ => return None,
    })
}

/// Whether `name` is one of the 14 `f16conv` instructions.
pub fn supports(name: &str) -> bool {
    spec(name).is_some()
}

fn read_src(
    wave: &Wave,
    kind: Src,
    code: u32,
    lane: usize,
    literal: Option<u32>,
    high: bool,
) -> Result<u32, Error> {
    if kind == Src::Word {
        return wave.read_lane(code, lane, literal);
    }
    if !(128..VGPR_FIRST).contains(&code) {
        let word = wave.read_lane(code, lane, literal)?;
        return Ok(if high { word >> 16 } else { word & HALF_MASK });
    }
    if kind == Src::Half {
        return read_half(wave, code, lane, literal, false);
    }
    Ok((wave.read_scalar(code, literal, 1)? as u32) & HALF_MASK)
}

fn modify(mut value: u32, kind: Src, abs_bit: bool, neg_bit: bool) -> u32 {
    if kind == Src::Int16 {
        return value;
    }
    let sign = if kind == Src::Word {
        FLOAT_SIGN
    } else {
        HALF_SIGN
    };
    if abs_bit {
        value &= !sign;
    }
    if neg_bit {
        value ^= sign;
    }
    value
}

fn write_dst(slot: &mut u32, dst: Dst, value: u32, high: bool) {
    let old = *slot;
    *slot = match dst {
        Dst::Word => value,
        _ if high => (old & HALF_MASK) | ((value & HALF_MASK) << 16),
        _ => (old & 0xFFFF_0000) | (value & HALF_MASK),
    };
}

fn check_vop3(
    name: &str,
    kinds: &[Src],
    dst: Dst,
    fields: &Fields,
    op_sel: u32,
) -> Result<(), Error> {
    if fields.omod != 0 {
        return Err(Error::Untranslated(format!(
            "{name}: omod not supported (not modelled)"
        )));
    }
    if fields.clamp && dst != Dst::Half {
        return Err(Error::Untranslated(format!(
            "{name}: clamp not supported on an integer or packed result"
        )));
    }
    if kinds.len() == 1 && op_sel != 0 {
        return Err(Error::Untranslated(format!(
            "{name}: op_sel not usable on a VOP1 opcode in VOP3 (ISA section 6.2.5)"
        )));
    }
    if (op_sel >> 3) & 1 != 0 && kinds.len() < 3 {
        return Err(Error::Untranslated(format!(
            "{name}: op_sel on the destination of a 32-bit result not supported"
        )));
    }
    if (op_sel >> 2) & 1 != 0 && kinds.len() < 3 {
        return Err(Error::Untranslated(format!(
            "{name}: op_sel on an unused source not supported"
        )));
    }
    for (index, &kind) in kinds.iter().enumerate() {
        if kind == Src::Word && (op_sel >> index) & 1 != 0 {
            return Err(Error::Untranslated(format!(
                "{name}: op_sel on a 32-bit source not supported"
            )));
        }
        if kind == Src::Int16 && ((fields.abs | fields.neg) >> index) & 1 != 0 {
            return Err(Error::Untranslated(format!(
                "{name}: abs/neg on an integer source not supported"
            )));
        }
        let code = fields.sources[index];
        if kind != Src::Word && (op_sel >> index) & 1 != 0 && (128..VGPR_FIRST).contains(&code) {
            return Err(Error::Untranslated(format!(
                "{name}: op_sel on the high half of a constant not supported"
            )));
        }
    }
    Ok(())
}

/// Executes a VOP1 unary `f16conv` instruction (`word` is the 32-bit encoding).
pub fn execute(wave: &mut Wave, name: &str, word: u32, literal: Option<u32>) -> Result<(), Error> {
    let src0 = word & 0x1FF;
    if EXTENSION_CODES.contains(&src0) {
        return Err(Error::Untranslated(format!(
            "{name}: SDWA/DPP form (src0 = 0x{src0:X}) not supported"
        )));
    }
    let (kinds, op, dst) = spec(name).ok_or_else(|| Error::Untranslated(name.to_string()))?;
    let vdst = ((word >> 17) & 0xFF) as usize;
    for lane in wave.active()? {
        let a = read_src(wave, kinds[0], src0, lane, literal, false)?;
        write_dst(&mut wave.v[vdst][lane], dst, op(a, 0, 0), false);
    }
    Ok(())
}

/// Executes a VOP3 `f16conv` instruction (`word` is the 64-bit encoding).
pub fn execute_vop3(
    wave: &mut Wave,
    name: &str,
    word: u64,
    literal: Option<u32>,
) -> Result<(), Error> {
    let (kinds, op, dst) = spec(name).ok_or_else(|| Error::Untranslated(name.to_string()))?;
    let fields = Fields::new(word);
    let op_sel = ((word >> 11) & 0xF) as u32;
    check_vop3(name, kinds, dst, &fields, op_sel)?;
    let high_dst = (op_sel >> 3) & 1 != 0;
    let vdst = fields.vdst as usize;
    for lane in wave.active()? {
        let mut args = [0u32; 3];
        for (index, &kind) in kinds.iter().enumerate() {
            let high_src = (op_sel >> index) & 1 != 0;
            let raw = read_src(wave, kind, fields.sources[index], lane, literal, high_src)?;
            args[index] = modify(
                raw,
                kind,
                (fields.abs >> index) & 1 != 0,
                (fields.neg >> index) & 1 != 0,
            );
        }
        let mut val = op(args[0], args[1], args[2]);
        if fields.clamp && dst == Dst::Half {
            val = half_clamp(val);
        }
        write_dst(&mut wave.v[vdst][lane], dst, val, high_dst);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_pack_and_med3_follow_the_model() {
        assert_eq!(normalized(0.5, UINT16_SCALE, false), 32768);
        assert_eq!(normalized(-1.0, INT16_SCALE, true), 0x8001);
        assert_eq!(med3(0x8000, 0x0000, 0x3C00), 0x0000);
        assert_eq!(med3(0x7E00, 0x4000, 0x3C00), 0x3C00);
    }
}
