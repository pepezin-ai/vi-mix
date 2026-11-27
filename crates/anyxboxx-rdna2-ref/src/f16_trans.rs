//! Half-precision (f16) transcendental instructions of VOP1 and VOP3 (`v_rcp_f16`, `v_sqrt_f16`,
//! `v_rsq_f16`, `v_log_f16`, `v_exp_f16`, `v_sin_f16`, `v_cos_f16`), ported from `emu/rdna2_exec_f16.py`.

use std::f64::consts::TAU;

use crate::error::Error;
use crate::half::{
    HALF_INFINITY, HALF_MASK, HALF_NAN, HALF_ONE, HALF_SIGN, half_clamp, half_from_f64,
    half_to_f64, read_half,
};
use crate::vop3::Fields;
use crate::wave::{VGPR_FIRST, Wave};

type UnaryF16 = fn(u32) -> u32;

fn half_rcp(bits: u32) -> u32 {
    let (sign, mag) = (bits & HALF_SIGN, bits & 0x7FFF);
    if mag > HALF_INFINITY {
        return HALF_NAN;
    }
    if mag == 0 {
        return sign | HALF_INFINITY;
    }
    if mag == HALF_INFINITY {
        return sign;
    }
    half_from_f64(1.0 / half_to_f64(bits))
}

fn half_sqrt(bits: u32) -> u32 {
    let mag = bits & 0x7FFF;
    if mag > HALF_INFINITY {
        return HALF_NAN;
    }
    if mag == 0 {
        return bits;
    }
    if bits & HALF_SIGN != 0 {
        return HALF_NAN;
    }
    if mag == HALF_INFINITY {
        return HALF_INFINITY;
    }
    half_from_f64(half_to_f64(bits).sqrt())
}

fn half_rsq(bits: u32) -> u32 {
    let (sign, mag) = (bits & HALF_SIGN, bits & 0x7FFF);
    if mag > HALF_INFINITY {
        return HALF_NAN;
    }
    if mag == 0 {
        return sign | HALF_INFINITY;
    }
    if sign != 0 {
        return HALF_NAN;
    }
    if mag == HALF_INFINITY {
        return 0;
    }
    half_from_f64(1.0 / half_to_f64(bits).sqrt())
}

fn half_log2(bits: u32) -> u32 {
    let mag = bits & 0x7FFF;
    if mag > HALF_INFINITY {
        return HALF_NAN;
    }
    if mag == 0 {
        return HALF_SIGN | HALF_INFINITY;
    }
    if bits & HALF_SIGN != 0 {
        return HALF_NAN;
    }
    if mag == HALF_INFINITY {
        return HALF_INFINITY;
    }
    if bits == HALF_ONE {
        return 0;
    }
    half_from_f64(half_to_f64(bits).log2())
}

fn half_exp2(bits: u32) -> u32 {
    let mag = bits & 0x7FFF;
    if mag > HALF_INFINITY {
        return HALF_NAN;
    }
    if mag == 0 {
        return HALF_ONE;
    }
    if bits == (HALF_SIGN | HALF_INFINITY) {
        return 0;
    }
    if bits == HALF_INFINITY {
        return HALF_INFINITY;
    }
    let value = half_to_f64(bits);
    if value >= 16.0 {
        return HALF_INFINITY;
    }
    if value <= -25.0 {
        return 0;
    }
    half_from_f64(value.exp2())
}

fn half_trig(bits: u32, is_cos: bool) -> u32 {
    let mag = bits & 0x7FFF;
    if mag >= HALF_INFINITY {
        return HALF_NAN;
    }
    if mag == 0 {
        return if is_cos { HALF_ONE } else { bits };
    }
    let value = half_to_f64(bits).abs();
    let neg = (bits & HALF_SIGN != 0) && !is_cos;
    let frac = value - value.trunc();
    let mut quadrant = (frac * 4.0).floor() as u32;
    let mut offset = frac - f64::from(quadrant) * 0.25;
    if is_cos {
        quadrant = (quadrant + 1) & 3;
    }
    if offset == 0.0 {
        if quadrant & 1 == 0 {
            return 0;
        }
        return if (quadrant == 3) ^ neg {
            HALF_SIGN | HALF_ONE
        } else {
            HALF_ONE
        };
    }
    if quadrant & 1 != 0 {
        offset = 0.25 - offset;
    }
    let result = if offset > 0.125 {
        ((0.25 - offset) * TAU).cos()
    } else {
        (offset * TAU).sin()
    };
    let encoded = half_from_f64(result);
    if encoded != 0 && ((quadrant >= 2) ^ neg) {
        encoded ^ HALF_SIGN
    } else {
        encoded
    }
}

fn operation(name: &str) -> Option<UnaryF16> {
    Some(match name {
        "v_rcp_f16" => half_rcp,
        "v_sqrt_f16" => half_sqrt,
        "v_rsq_f16" => half_rsq,
        "v_log_f16" => half_log2,
        "v_exp_f16" => half_exp2,
        "v_sin_f16" => |a| half_trig(a, false),
        "v_cos_f16" => |a| half_trig(a, true),
        _ => return None,
    })
}

/// Whether `name` is one of the seven scalar f16 transcendental instructions.
pub fn supports(name: &str) -> bool {
    operation(name).is_some()
}

fn read_src(
    wave: &Wave,
    name: &str,
    code: u32,
    lane: usize,
    literal: Option<u32>,
    high: bool,
) -> Result<u32, Error> {
    if high && (128..VGPR_FIRST).contains(&code) {
        return Err(Error::Untranslated(format!(
            "{name}: op_sel on the high half of a constant not supported"
        )));
    }
    read_half(wave, code, lane, literal, high)
}

/// Executes a VOP1 f16 transcendental (`word` is the 32-bit encoding; SDWA/DPP already refused).
pub fn execute(wave: &mut Wave, name: &str, word: u32, literal: Option<u32>) -> Result<(), Error> {
    let op = operation(name).ok_or_else(|| Error::Untranslated(name.to_string()))?;
    let src0 = word & 0x1FF;
    let vdst = ((word >> 17) & 0xFF) as usize;
    for lane in wave.active()? {
        let a = read_src(wave, name, src0, lane, literal, false)?;
        let old = wave.v[vdst][lane];
        wave.v[vdst][lane] = (old & 0xFFFF_0000) | op(a);
    }
    Ok(())
}

/// Executes a VOP3 f16 transcendental (`word` is the 64-bit encoding).
pub fn execute_vop3(
    wave: &mut Wave,
    name: &str,
    word: u64,
    literal: Option<u32>,
) -> Result<(), Error> {
    let op = operation(name).ok_or_else(|| Error::Untranslated(name.to_string()))?;
    let fields = Fields::new(word);
    if fields.omod != 0 {
        return Err(Error::Untranslated(format!(
            "{name}: omod on an f16 result not supported"
        )));
    }
    let op_sel = ((word >> 11) & 0xF) as u32;
    let high_src = op_sel & 1 != 0;
    let high_dst = (op_sel >> 3) & 1 != 0;
    let vdst = fields.vdst as usize;
    for lane in wave.active()? {
        let mut a = read_src(wave, name, fields.sources[0], lane, literal, high_src)?;
        if fields.abs & 1 != 0 {
            a &= !HALF_SIGN;
        }
        if fields.neg & 1 != 0 {
            a ^= HALF_SIGN;
        }
        let mut res = op(a);
        if fields.clamp {
            res = half_clamp(res);
        }
        let old = wave.v[vdst][lane];
        wave.v[vdst][lane] = if high_dst {
            (old & HALF_MASK) | (res << 16)
        } else {
            (old & 0xFFFF_0000) | res
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_points_match_the_isa_and_python_model() {
        assert_eq!(half_rcp(0x0000), HALF_INFINITY);
        assert_eq!(half_rcp(0x8000), HALF_SIGN | HALF_INFINITY);
        assert_eq!(half_sqrt(0x8000), 0x8000);
        assert_eq!(half_sqrt(0xBC00), HALF_NAN);
        assert_eq!(half_rsq(0x4000), half_from_f64(0.5f64.sqrt()));
        assert_eq!(half_log2(HALF_ONE), 0x0000);
        assert_eq!(half_log2(0x8000), HALF_SIGN | HALF_INFINITY);
        assert_eq!(half_exp2(0x8000), HALF_ONE);
        assert_eq!(half_exp2(HALF_SIGN | HALF_INFINITY), 0x0000);
        assert_eq!(half_trig(0x8000, false), 0x8000); // sin(-0) = -0
        assert_eq!(half_trig(0x7BFF, false), 0x0000); // sin(65504) = +0
        assert_eq!(half_trig(0x7BFF, true), HALF_ONE); // cos(65504) = +1
        assert_eq!(half_trig(0x3400, true), 0x0000); // cos(0.25) = +0
    }
}
