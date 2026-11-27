//! Non-translation diagnostic/test switches (such as `VICTORY_D3D12_DEBUG`,
//! `VICTORY_DESC_DATA`, `VICTORY_WAVE_WHY`, `VICTORY_SHADER_CENSUS`) must not
//! enter `translation_fingerprint()`, or enabling them invalidates the entire
//! shader cache on disk. Real translation switches must still change it.

#[test]
fn fingerprint_ignores_non_translation_switches() {
    let before = victory_shader::emit::translation_fingerprint();

    let ignored = [
        "VICTORY_D3D12_DEBUG",
        "VICTORY_DESC_DATA",
        "VICTORY_DESC_WHY",
        "VICTORY_WAVE_WHY",
        "VICTORY_SHADER_CENSUS",
        "VICTORY_WARP",
        "VICTORY_DXIL",
        "VICTORY_EMIT_DUMP",
        "VICTORY_DIFF_ONLY",
        "VICTORY_REF_VS",
        "VICTORY_REF_PS",
    ];
    let saved: Vec<_> = ignored
        .iter()
        .map(|&name| (name, std::env::var_os(name)))
        .collect();

    for &name in &ignored {
        // SAFETY: isolated integration test binary with a single test function.
        unsafe { std::env::set_var(name, "1") };
    }
    let after_ignored = victory_shader::emit::translation_fingerprint();

    for (name, prev) in saved {
        // SAFETY: restoring env in this isolated single-test binary.
        unsafe {
            match prev {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }
    }

    assert_eq!(
        after_ignored, before,
        "non-translation switches changed translation_fingerprint() and would invalidate the shader cache"
    );

    // Real translation switches must still change the cache key.
    let prev_f16 = std::env::var_os("VICTORY_F16_NEAREST");
    // SAFETY: isolated integration test binary with a single test function.
    unsafe { std::env::set_var("VICTORY_F16_NEAREST", "1") };
    let after_real = victory_shader::emit::translation_fingerprint();
    // SAFETY: restoring env in this isolated single-test binary.
    unsafe {
        match prev_f16 {
            Some(v) => std::env::set_var("VICTORY_F16_NEAREST", v),
            None => std::env::remove_var("VICTORY_F16_NEAREST"),
        }
    }
    assert_ne!(
        after_real, before,
        "VICTORY_F16_NEAREST must still participate in translation_fingerprint()"
    );
}
