//! Fingerprint of the translator's own source, for the shader cache key
//! (`victory-gpu`'s `shader_cache`): a rebuilt executable whose translator did
//! not change keeps its cached translations. Also lists every `VICTORY_*`
//! switch the translator reads, whose values join the key at run time.
//! Reads only this crate's `src` folder; writes nothing but cargo directives.

use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// FNV-1a, 64 bits, two lanes with different offsets: a mismatch only costs
/// a cache miss, so this need not be cryptographic.
fn fnv(state: &mut [u64; 2], bytes: &[u8]) {
    for &b in bytes {
        for (i, s) in state.iter_mut().enumerate() {
            *s ^= u64::from(b) ^ (i as u64 * 0x9e);
            *s = s.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// Source files that do not affect translated DXIL (unit-test modules, orphan
/// test files, standalone D3D12 test host, DXIL validator wrapper, and
/// instruction census logging). Changes or env switches in these files must not
/// invalidate the shader cache.
fn skip_file(rel: &str) -> bool {
    rel.ends_with("_tests.rs")
        || matches!(
            rel,
            "emit/host.rs" | "emit/validator.rs" | "emit/bitcode/census.rs"
        )
}

/// `VICTORY_*` string literals inside translator source files that do not
/// change translated DXIL: compile-time `env!()` keys, explicitly listed
/// diagnostic-only loggers (never a wildcard suffix, so new switches fail
/// closed into the cache key), and test-only reference shader overrides inside
/// `mod tests`.
fn skip_switch(name: &str) -> bool {
    matches!(
        name,
        "VICTORY_DESC_WHY"
            | "VICTORY_WAVE_WHY"
            | "VICTORY_SHADER_SOURCE_HASH"
            | "VICTORY_SHADER_SWITCHES"
            | "VICTORY_REF_VS"
            | "VICTORY_REF_PS"
    )
}

fn main() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let mut list = Vec::new();
    files(&src, &mut list);
    list.sort();
    let mut state = [0xcbf2_9ce4_8422_2325u64, 0x8422_2325_cbf2_9ce4u64];
    let mut switches = std::collections::BTreeSet::new();
    for p in &list {
        let rel = p.strip_prefix(&src).unwrap_or(p).to_string_lossy().replace('\\', "/");
        if skip_file(&rel) {
            continue;
        }
        let text = std::fs::read(p).unwrap_or_default();
        fnv(&mut state, rel.as_bytes());
        fnv(&mut state, &(text.len() as u64).to_le_bytes());
        fnv(&mut state, &text);
        let s = String::from_utf8_lossy(&text);
        let mut rest = &s[..];
        while let Some(at) = rest.find("\"VICTORY_") {
            let tail = &rest[at + 1..];
            let end = tail
                .find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
                .unwrap_or(tail.len());
            if tail[end..].starts_with('"') {
                let name = &tail[..end];
                if !skip_switch(name) {
                    switches.insert(name.to_string());
                }
            }
            rest = &tail[end..];
        }
    }
    println!("cargo:rustc-env=VICTORY_SHADER_SOURCE_HASH={:016x}{:016x}", state[0], state[1]);
    let joined: Vec<_> = switches.into_iter().collect();
    println!("cargo:rustc-env=VICTORY_SHADER_SWITCHES={}", joined.join(","));
}
