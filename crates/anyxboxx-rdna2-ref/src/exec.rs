//! Running a program: one instruction at a time, until s_endpgm or the end of the binary (port of
//! `step` and `run` of `emu/rdna2_exec.py`).
//!
//! Families: scalar (SOP1, SOP2, SOPC, SOPK, SOPP) and branches, the vector encodings, the memory
//! families, MIMG, EXP and VINTRP, each in its own module. What a module does not execute refuses with
//! `Untranslated`, which names the instruction, as the Python does for the instructions it has not
//! written.

use crate::compare;
use crate::decode::{Classified, DWORD, classify, mnemonic};
use crate::ds;
use crate::error::Error;
use crate::export;
use crate::ext;
use crate::ext_compare;
use crate::flat;
use crate::format_exec;
use crate::image_exec;
use crate::interp;
use crate::mubuf;
use crate::pc;
use crate::scalar;
use crate::smem;
use crate::vector;
use crate::vop3;
use crate::vop3p;
use crate::wave::{MAX_STEPS, Wave};

/// The families this module executes.
const SCALAR_FAMILIES: [&str; 5] = ["SOP1", "SOP2", "SOPC", "SOPK", "SOPP"];
const VECTOR_FAMILIES: [&str; 2] = ["VOP1", "VOP2"];
/// The source codes that announce an SDWA or DPP word.
const EXTENSION_CODES: [u32; 4] = [0xE9, 0xEA, 0xF9, 0xFA];

/// Runs one VOP1 or VOP2 instruction: no-ops, the carry chain, the vector table, or the SDWA and DPP
/// forms when the source code announces an extension word (the literal slot holds that word).
fn execute_vector(
    wave: &mut Wave,
    name: &str,
    family: &str,
    word: u32,
    literal: Option<u32>,
) -> Result<bool, Error> {
    if vector::is_no_operation(name) {
        return Ok(true);
    }
    if crate::f16::supports(name) {
        crate::f16::execute(wave, name, family, word, literal)?;
        return Ok(true);
    }
    let fields = vector::fields(family, word);
    let carry = family == "VOP2" && vector::is_carry(name);
    if EXTENSION_CODES.contains(&(word & 0x1FF)) {
        if crate::wide::spec(name).is_some()
            || crate::movrel::is_vector(name)
            || crate::f16_trans::supports(name)
        {
            // 64-bit, movrel and f16 transcendental operations take no extension word: refused before any lane.
            return Err(Error::Untranslated(format!(
                "instruction non traduite : {name} (SDWA/DPP)"
            )));
        }
        if carry {
            return Ok(false);
        }
        let ext = literal.ok_or_else(|| Error::Operand("mot d'extension absent".into()))?;
        ext::execute(wave, name, &fields, ext)?;
        return Ok(true);
    }
    if family == "VOP1" && crate::movrel::is_vector(name) {
        crate::movrel::vector(wave, name, fields.vdst, fields.src0, literal)?;
        return Ok(true);
    }
    if family == "VOP1" && crate::wide::spec(name).is_some() {
        crate::wide::execute(
            wave,
            name,
            &[fields.src0],
            fields.vdst,
            literal,
            0,
            0,
            false,
            0,
        )?;
        return Ok(true);
    }
    if family == "VOP1" && crate::f16_trans::supports(name) {
        crate::f16_trans::execute(wave, name, word, literal)?;
        return Ok(true);
    }
    if carry {
        return vector::execute_carry(wave, name, &fields, literal);
    }
    vector::execute(wave, name, &fields, literal)
}

/// Runs one VOPC instruction. The integer 16-bit and 32-bit compares take SDWA and refuse DPP16
/// and DPP8; the 64-bit and class compares read an extension code as a plain source, which fails on the
/// first active lane, as in the Python (they are dispatched before its SDWA and DPP check).
fn execute_compare(
    wave: &mut Wave,
    name: &str,
    word: u32,
    literal: Option<u32>,
) -> Result<bool, Error> {
    let (src0, vsrc1) = (word & 0x1FF, (word >> 9) & 0xFF);
    if crate::cmpf16::supports(name) {
        crate::cmpf16::execute(wave, name, src0, vsrc1, literal)?;
        return Ok(true);
    }
    if EXTENSION_CODES.contains(&src0) && ext_compare::supports(name) {
        if src0 != ext::SDWA {
            return Err(ext_compare::refuse_dpp(name, src0 == ext::DPP16));
        }
        let ext = literal.ok_or_else(|| Error::Operand("mot d'extension absent".into()))?;
        ext_compare::execute(wave, name, vsrc1, ext)?;
        return Ok(true);
    }
    compare::execute(wave, name, src0, vsrc1, literal)
}

/// Branches of the Python `BRANCHES` table: the condition, read from the wave.
fn branch_taken(name: &str, wave: &Wave) -> Result<Option<bool>, Error> {
    let taken = match name {
        "s_branch" => true,
        "s_cbranch_scc0" => !wave.scc,
        "s_cbranch_scc1" => wave.scc,
        "s_cbranch_vccz" => wave.vcc()? == 0,
        "s_cbranch_vccnz" => wave.vcc()? != 0,
        "s_cbranch_execz" => wave.exec()? == 0,
        "s_cbranch_execnz" => wave.exec()? != 0,
        _ => return Ok(None),
    };
    Ok(Some(taken))
}

fn word_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Executes the instruction at `wave.pc`. `Ok(false)` after s_endpgm.
pub fn step(wave: &mut Wave, data: &[u8]) -> Result<bool, Error> {
    let Classified {
        family,
        opcode,
        size,
    } = classify(data, wave.pc);
    let name =
        mnemonic(family, opcode).map_or_else(|| format!("{family}#{opcode}"), str::to_string);
    if name == "s_endpgm" {
        return Ok(false);
    }
    let refused = || {
        Error::Untranslated(format!(
            "instruction non traduite : {name} ({family}, opcode 0x{opcode:X})"
        ))
    };
    let vector_family = VECTOR_FAMILIES.contains(&family);
    let compare_family = family == "VOPC";
    let vop3_family = family == "VOP3";
    let vop3p_family = family == "VOP3P";
    let smem_family = family == "SMEM";
    let ds_family = family == "DS";
    let flat_family = flat::is_family(family);
    let mubuf_family = family == "MUBUF";
    // Typed accesses (MUBUF buffer_*_format, MTBUF): the other MTBUF forms (_d16) are refused.
    let format_family = (mubuf_family || family == "MTBUF") && format_exec::supports(&name);
    // MIMG: the 2D loads, stores and samples of `image_exec`; the other image instructions are refused.
    let image_family = family == "MIMG" && image_exec::supports(&name);
    // EXP: its one opcode, `exp` (the Python `rdna2_exec_graphics`).
    let export_family = family == "EXP" && name == "exp";
    // VINTRP: v_interp_p1_f32, v_interp_p2_f32, v_interp_mov_f32 (opcode 3 has no name and is refused).
    let interp_family = family == "VINTRP" && interp::supports(&name);
    if !SCALAR_FAMILIES.contains(&family)
        && !vector_family
        && !compare_family
        && !vop3_family
        && !vop3p_family
        && !smem_family
        && !ds_family
        && !flat_family
        && !mubuf_family
        && !format_family
        && !image_family
        && !export_family
        && !interp_family
    {
        return Err(refused());
    }
    if wave.pc + size * DWORD > data.len() {
        return Err(Error::Operand(format!(
            "instruction tronquée à l'octet {}",
            wave.pc
        )));
    }
    let word = word_at(data, wave.pc);
    wave.steps += 1;
    let literal = (size > 1).then(|| word_at(data, wave.pc + DWORD));
    let fields = scalar::fields(word);
    let next_pc = wave.pc + size * DWORD;
    if ds_family {
        // Two words: the offsets, the addresses and the data registers are in the second.
        let wide = u64::from(word) | u64::from(word_at(data, wave.pc + DWORD)) << 32;
        ds::execute(wave, &name, wide)?;
        wave.pc = next_pc;
        return Ok(true);
    }
    if family == "VOP2" && name == "v_pk_fmac_f16" {
        // Ahead of the generic VOP2 path: the packed form refuses its own SDWA and DPP sources.
        crate::pkf16::execute_fmac_vop2(wave, word, literal)?;
        wave.pc = next_pc;
        return Ok(true);
    }
    if flat_family {
        // Two words: the offset, the registers and the GLC bit are in the first, the registers in the second.
        let wide = u64::from(word) | u64::from(word_at(data, wave.pc + DWORD)) << 32;
        flat::execute(wave, family, &name, wide)?;
        wave.pc = next_pc;
        return Ok(true);
    }
    if image_family {
        // Two words, then the NSA words (bits 1-2 of the first): `size` counts them all.
        let words: Vec<u32> = (0..size)
            .map(|k| word_at(data, wave.pc + k * DWORD))
            .collect();
        image_exec::execute(wave, &name, &words)?;
        wave.pc = next_pc;
        return Ok(true);
    }
    if export_family {
        // Two words: the enable bits, the target and the flags in the first, the sources in the second.
        let wide = u64::from(word) | u64::from(word_at(data, wave.pc + DWORD)) << 32;
        export::execute(wave, wide)?;
        wave.pc = next_pc;
        return Ok(true);
    }
    if interp_family {
        // One word.
        interp::execute(wave, &name, word)?;
        wave.pc = next_pc;
        return Ok(true);
    }
    if format_family {
        // Two words, as MUBUF; MTBUF adds the format (bits 19-25) to the first.
        let wide = u64::from(word) | u64::from(word_at(data, wave.pc + DWORD)) << 32;
        format_exec::execute(wave, &name, wide)?;
        wave.pc = next_pc;
        return Ok(true);
    }
    if mubuf_family {
        // Two words: the offset and the flags are in the first, the registers and the descriptor in the second.
        let wide = u64::from(word) | u64::from(word_at(data, wave.pc + DWORD)) << 32;
        mubuf::execute(wave, &name, wide)?;
        wave.pc = next_pc;
        return Ok(true);
    }
    if smem_family {
        // Two words: the second holds the offset (smem.rs decodes both).
        let wide = u64::from(word) | u64::from(word_at(data, wave.pc + DWORD)) << 32;
        smem::execute(wave, &name, wide)?;
        wave.pc = next_pc;
        return Ok(true);
    }
    if vop3_family {
        // The 64-bit word: the first word is the low half. The literal, when present, is the third word.
        let wide = u64::from(word) | u64::from(word_at(data, wave.pc + DWORD)) << 32;
        let literal3 = (size > 2).then(|| word_at(data, wave.pc + 2 * DWORD));
        if !vop3::execute(wave, &name, wide, literal3)? {
            return Err(refused());
        }
        wave.pc = next_pc;
        return Ok(true);
    }
    if vop3p_family {
        let wide = u64::from(word) | u64::from(word_at(data, wave.pc + DWORD)) << 32;
        let literal3 = (size > 2).then(|| word_at(data, wave.pc + 2 * DWORD));
        if !vop3p::execute(wave, &name, wide, literal3)? {
            return Err(refused());
        }
        wave.pc = next_pc;
        return Ok(true);
    }
    if compare_family {
        if !execute_compare(wave, &name, word, literal)? {
            return Err(refused());
        }
        wave.pc = next_pc;
        return Ok(true);
    }
    if family == "VOP1" && crate::f16conv::supports(&name) {
        crate::f16conv::execute(wave, &name, word, literal)?;
        wave.pc = next_pc;
        return Ok(true);
    }
    if vector_family {
        if !execute_vector(wave, &name, family, word, literal)? {
            return Err(refused());
        }
        wave.pc = next_pc;
        return Ok(true);
    }
    if let Some(taken) = branch_taken(&name, wave)? {
        wave.pc = if taken {
            let offset = i64::from(fields.simm16 as u16 as i16) * DWORD as i64;
            (wave.pc as i64 + DWORD as i64 + offset) as usize
        } else {
            next_pc
        };
        return Ok(true);
    }
    if pc::family_of(&name) == Some(family) {
        // s_getpc_b64, s_setpc_b64, s_swappc_b64 (SOP1) and s_call_b64 (SOPK): after the branches, as in
        // the Python.
        wave.pc = pc::execute(wave, &name, &fields, next_pc, data.len())?;
        return Ok(true);
    }
    if !scalar::execute(wave, &name, &fields, literal)? {
        return Err(refused());
    }
    wave.pc = next_pc;
    Ok(true)
}

/// Runs a program until s_endpgm, or until its bytes end. Its state is left in `wave`.
pub fn run(data: &[u8], wave: &mut Wave) -> Result<(), Error> {
    for _ in 0..MAX_STEPS {
        if wave.pc + DWORD > data.len() || !step(wave, data)? {
            return Ok(());
        }
    }
    Err(Error::Loop(MAX_STEPS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(words: &[u32]) -> Vec<u8> {
        let mut all = words.to_vec();
        all.push(0x17F << 23 | 1 << 16); // s_endpgm
        all.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    #[test]
    fn a_literal_mov_and_an_add_run_to_the_end() {
        // s_mov_b32 s0, 0x12345678 ; s_add_u32 s1, s0, 1
        let data = program(&[
            0x17D << 23 | 3 << 8 | 255,
            0x1234_5678,
            0b10 << 30 | 1 << 16,
        ]);
        let mut wave = Wave::new(32).unwrap();
        wave.s[0] = 0;
        run(&data, &mut wave).unwrap();
        assert_eq!(wave.s[0], 0x1234_5678);
        assert_eq!(wave.steps, 2);
    }

    #[test]
    fn a_branch_skips_the_instructions_it_jumps_over() {
        // s_branch +1 ; s_mov_b32 s0, 1 (skipped) ; s_mov_b32 s1, 2
        let data = program(&[
            (0x17F << 23) | (2 << 16) | 1,
            (0x17D << 23) | (3 << 8) | (128 + 1),
            (0x17D << 23) | (1 << 16) | (3 << 8) | (128 + 2),
        ]);
        let mut wave = Wave::new(32).unwrap();
        run(&data, &mut wave).unwrap();
        assert_eq!((wave.s[0], wave.s[1]), (0, 2));
    }

    #[test]
    fn an_unknown_instruction_is_named_as_untranslated() {
        let data = program(&[0x35 << 26 | 0x3FF << 16, 0]); // a VOP3 opcode no table names
        let mut wave = Wave::new(32).unwrap();
        match run(&data, &mut wave) {
            Err(Error::Untranslated(message)) => assert!(message.contains("VOP3"), "{message}"),
            other => panic!("expected untranslated, got {other:?}"),
        }
    }
}
