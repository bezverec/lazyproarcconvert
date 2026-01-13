// src/manifest.rs
use std::{
    fs,
    path::Path,
};

use anyhow::{Context, Result};
use chrono::Local;
use serde::Serialize;

use crate::blake3::compute_blake3;
use crate::collect_tiffs_in_dir;

/// Informace o jednom souboru
#[derive(Debug, Serialize, Clone)]
pub struct FileInfo {
    pub path: String,   // plná filesystem cesta
    pub size: u64,
    pub blake3: String,
}

/// Informace o jedné stránce / indexu
#[derive(Debug, Serialize, Clone)]
pub struct PageEntry {
    pub index: String,           // např. "0001"
    pub original_tiff: FileInfo, // ZMĚNA: z 'tiff' na 'original_tiff'
    pub ac_jp2: Option<FileInfo>,
    pub uc_jp2: Option<FileInfo>,
    pub txt: Option<FileInfo>,
    pub alto: Option<FileInfo>,
}

/// Manifest celé dávky - PŘIDÁNÁ NOVÁ POLE podle main.rs
#[derive(Debug, Serialize, Clone)]
pub struct BatchManifest {
    pub batch_name: String,
    pub start_index: u32,    // PŘIDÁNO: počáteční index
    pub file_count: usize,   // PŘIDÁNO: počet souborů
    pub input_dir: String,
    pub output_dir: String,
    pub logs_dir: String,
    pub created_at: String,
    pub generated: String,   // PŘIDÁNO: timestamp generování
    pub lang: String,
    pub alto_version: String,
    pub pages: Vec<PageEntry>,
}

/// Postaví manifest pro jednu dávku a vrátí ho.
/// Logs adresář se vytvoří, ale manifest/checksums se zatím nezapisují.
pub fn build_manifest_for_batch(
    batch_name: &str,
    input_dir: &Path,
    output_dir: &Path,
    logs_dir: &Path,
    index_start: u32,
    digits: usize,
    do_master: bool,
    do_user: bool,
    do_txt: bool,
    do_alto: bool,
    lang: &str,
    alto_version: &str,
) -> Result<BatchManifest> {
    fs::create_dir_all(logs_dir)
        .with_context(|| format!("Nelze vytvořit logs adresář `{}`", logs_dir.display()))?;

    // TIFFy ve stejné logice jako v process_batch
    let tiffs = collect_tiffs_in_dir(input_dir)
        .with_context(|| format!("Nelze znovu načíst TIFFy z `{}`", input_dir.display()))?;

    let mut pages = Vec::new();
    let mut idx = index_start;

    for tif in tiffs {
        let index_str = format!("{idx:0digits$}");

        let tiff_info = file_info(&tif)
            .with_context(|| format!("BLAKE3 pro `{}` selhal", tif.display()))?;

        let ac_jp2 = if do_master {
            let p = output_dir.join(format!("{index_str}.ac.jp2"));  // ZMĚNA: xxxx.ac.jp2
            if p.exists() {
                Some(file_info(&p).with_context(|| format!("BLAKE3 pro `{}` selhal", p.display()))?)
            } else {
                None
            }
        } else {
            None
        };

        let uc_jp2 = if do_user {
            let p = output_dir.join(format!("{index_str}.uc.jp2"));  // ZMĚNA: xxxx.uc.jp2
            if p.exists() {
                Some(file_info(&p).with_context(|| format!("BLAKE3 pro `{}` selhal", p.display()))?)
            } else {
                None
            }
        } else {
            None
        };

        let txt = if do_txt {
            let p = output_dir.join(format!("{index_str}.ocr.txt"));  // ZMĚNA: xxxx.ocr.txt
            if p.exists() {
                Some(file_info(&p).with_context(|| format!("BLAKE3 pro `{}` selhal", p.display()))?)
            } else {
                None
            }
        } else {
            None
        };

        let alto = if do_alto {
            let p = output_dir.join(format!("{index_str}.ocr.xml"));  // ZMĚNA: xxxx.ocr.xml
            if p.exists() {
                Some(file_info(&p).with_context(|| format!("BLAKE3 pro `{}` selhal", p.display()))?)
            } else {
                None
            }
        } else {
            None
        };

        pages.push(PageEntry {
            index: index_str,
            original_tiff: tiff_info, // ZMĚNA: z 'tiff' na 'original_tiff'
            ac_jp2,
            uc_jp2,
            txt,
            alto,
        });

        idx += 1;
    }

    let manifest = BatchManifest {
        batch_name: batch_name.to_string(),
        start_index: index_start, // PŘIDÁNO
        file_count: pages.len(),  // PŘIDÁNO
        input_dir: input_dir.to_string_lossy().to_string(),
        output_dir: output_dir.to_string_lossy().to_string(),
        logs_dir: logs_dir.to_string_lossy().to_string(),
        created_at: Local::now().to_rfc3339(),
        generated: Local::now().to_rfc3339(), // PŘIDÁNO
        lang: lang.to_string(),
        alto_version: alto_version.to_string(),
        pages,
    };

    Ok(manifest)
}

/// Spočítá FileInfo pro daný soubor
fn file_info(path: &Path) -> Result<FileInfo> {
    let meta = fs::metadata(path)?;
    let size = meta.len();
    let hash = compute_blake3(path)?;
    Ok(FileInfo {
        path: path.to_string_lossy().to_string(),
        size,
        blake3: hash,
    })
}

/// Zapíše manifest.json a checksums.txt do logs_dir
pub fn write_manifest_and_checksums(
    manifest: &BatchManifest,
    logs_dir: &Path,
) -> Result<()> {
    // manifest.json
    let manifest_json = serde_json::to_string_pretty(manifest)?;
    fs::write(logs_dir.join("manifest.json"), manifest_json)?;

    // checksums.txt (jednoduchá verze, bez logu)
    let mut checksums = String::new();
    let mut add = |fi: &FileInfo| {
        checksums.push_str(&format!("{}  {}\n", fi.blake3, fi.path));
    };

    for page in &manifest.pages {
        add(&page.original_tiff); // ZMĚNA: z 'tiff' na 'original_tiff'
        if let Some(ref f) = page.ac_jp2 {
            add(f);
        }
        if let Some(ref f) = page.uc_jp2 {
            add(f);
        }
        if let Some(ref f) = page.txt {
            add(f);
        }
        if let Some(ref f) = page.alto {
            add(f);
        }
    }

    fs::write(logs_dir.join("checksums.txt"), checksums)?;
    Ok(())
}