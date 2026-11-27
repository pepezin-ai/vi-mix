//! Bitwise round-to-nearest-even for the DXIL emitter.

use super::{Bin, Lower, V, pred};

impl Lower<'_> {
    /// Round `f` to the nearest integer, ties to even, in branchless integer
    /// bit arithmetic: NVIDIA driver 621.xx (measured on RTX 5070 Ti 621.40)
    /// constant-folds DXIL `Round_ne` (opcode 26) of half-way literals such as
    /// `2.5` to `3.0` instead of `2.0` after a barrier, and replaces NaN
    /// payloads with `0x7FFFFFFF`. Computing IEEE-754 ties-to-even on the bits
    /// matches RDNA 2 on NVIDIA and WARP (including `-0.0` and quieted NaNs).
    pub(super) fn rndne_f32(&mut self, f: V) -> V {
        let c = self.cur;
        let k = |l: &mut Self, x: u32| l.s.m.i32(x);
        let a = self.as_int(f);
        let (sign_mask, mag_mask, qnan_bit, inf_bits) = (
            k(self, 0x8000_0000),
            k(self, 0x7FFF_FFFF),
            k(self, 0x0040_0000),
            k(self, 0x7F80_0000),
        );
        let (two23, half, one_f) = (k(self, 0x4B00_0000), k(self, 0x3F00_0000), k(self, 0x3F80_0000));
        let (k0, k1, k23, k31, k150) = (k(self, 0), k(self, 1), k(self, 23), k(self, 31), k(self, 150));

        let sign = self.s.m.bin(c, Bin::And, a, sign_mask);
        let mag = self.s.m.bin(c, Bin::And, a, mag_mask);

        // 1.0 <= |x| < 2^23: shift = 150 - biased_exp in 1..=23 (masked to 0..=31
        // so out-of-range lanes never shift by >= 32).
        let exp = self.s.m.bin(c, Bin::LShr, mag, k23);
        let shift_raw = self.s.m.bin(c, Bin::Sub, k150, exp);
        let shift = self.s.m.bin(c, Bin::And, shift_raw, k31);
        let half_shift_raw = self.s.m.bin(c, Bin::Sub, shift, k1);
        let half_shift = self.s.m.bin(c, Bin::And, half_shift_raw, k31);
        let unit = self.s.m.bin(c, Bin::Shl, k1, shift);
        let frac_mask = self.s.m.bin(c, Bin::Sub, unit, k1);
        let half_bit = self.s.m.bin(c, Bin::Shl, k1, half_shift);
        let rem = self.s.m.bin(c, Bin::And, mag, frac_mask);
        let ones = k(self, u32::MAX);
        let not_frac = self.s.m.bin(c, Bin::Xor, frac_mask, ones);
        let trunc = self.s.m.bin(c, Bin::And, mag, not_frac);
        let gt_half = self.s.m.cmp(c, pred::IUGT, rem, half_bit);
        let eq_half = self.s.m.cmp(c, pred::EQ, rem, half_bit);
        let lsb = self.s.m.bin(c, Bin::And, trunc, unit);
        let odd = self.s.m.cmp(c, pred::NE, lsb, k0);
        let tie_up = self.s.m.bin(c, Bin::And, eq_half, odd);
        let inc = self.s.m.bin(c, Bin::Or, gt_half, tie_up);
        let add = self.s.m.select(c, inc, unit, k0);
        let rounded_mag = self.s.m.bin(c, Bin::Add, trunc, add);
        let mid = self.s.m.bin(c, Bin::Or, sign, rounded_mag);

        // 0.5 < |x| < 1.0 -> ±1.0; |x| <= 0.5 -> ±0.0.
        let lt_one = self.s.m.cmp(c, pred::IULT, mag, one_f);
        let le_half = self.s.m.cmp(c, pred::IULE, mag, half);
        let signed_one = self.s.m.bin(c, Bin::Or, sign, one_f);
        let small = self.s.m.select(c, le_half, sign, signed_one);
        let r = self.s.m.select(c, lt_one, small, mid);

        // |x| >= 2^23: integers and infinities stay unchanged; NaNs quiet bit 22.
        let ge_two23 = self.s.m.cmp(c, pred::IUGE, mag, two23);
        let is_nan = self.s.m.cmp(c, pred::IUGT, mag, inf_bits);
        let quiet = self.s.m.bin(c, Bin::Or, a, qnan_bit);
        let large = self.s.m.select(c, is_nan, quiet, a);
        let bits = self.s.m.select(c, ge_two23, large, r);
        self.as_float(bits)
    }
}
