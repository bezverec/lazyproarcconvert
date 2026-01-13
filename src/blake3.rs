// src/blake3.rs
use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result};

pub fn compute_blake3(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("Nelze otevřít `{}` pro BLAKE3", path.display()))?;

    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 8192];

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

// Alias pro kompatibilitu s main.rs
pub fn hash_file(path: &Path) -> Result<String> {
    compute_blake3(path)
}