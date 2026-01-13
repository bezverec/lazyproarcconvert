use anyhow::{anyhow, Context, Result};
use image::GenericImageView;
use serde::Deserialize;
use webp;
use std::fs;
use std::path::{Path, PathBuf};

/// Struktura jednoho souboru v manifestu
#[derive(Debug, Deserialize)]
struct ManifestFile {
    path: String,
    size: u64,
    blake3: String,
}

/// Jedna stránka v manifestu
#[derive(Debug, Deserialize)]
struct ManifestPage {
    index: String,
    
    // Podpora pro starý i nový název
    #[serde(alias = "original_tiff", rename = "tiff")]
    tiff: ManifestFile,
    
    #[serde(rename = "ac_jp2")]
    ac_jp2: Option<ManifestFile>,
    
    #[serde(rename = "uc_jp2")]
    uc_jp2: Option<ManifestFile>,
    
    txt: Option<ManifestFile>,
    alto: Option<ManifestFile>,
}

/// Minimalní manifest pro náhledy
#[derive(Debug, Deserialize)]
struct Manifest {
    pages: Vec<ManifestPage>,
}

/// Vygeneruje WebP náhledy přímo z TIFF souborů
pub fn generate_webp_previews(logs_dir: &Path) -> Result<()> {
    let manifest_path = logs_dir.join("manifest.json");

    if !manifest_path.exists() {
        return Err(anyhow!(
            "Manifest neexistuje: {}",
            manifest_path.display()
        ));
    }

    let manifest_json = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Nelze načíst manifest.json: {}", manifest_path.display()))?;

    let manifest: Manifest = serde_json::from_str(&manifest_json)
        .with_context(|| format!("Nelze parsovat manifest.json: {}", manifest_path.display()))?;

    let mut generated = 0usize;
    let mut errors = 0usize;

    for page in &manifest.pages {
        // Používáme přímo TIFF soubory z manifestu
        let tiff_path = PathBuf::from(&page.tiff.path);
        
        if !tiff_path.exists() {
            eprintln!(
                "WebP: TIFF pro stránku {} neexistuje: {} – přeskočeno.",
                page.index,
                tiff_path.display()
            );
            errors += 1;
            continue;
        }

        // Cíl: page_XXXX.webp v logs_dir
        let webp_name = format!("page_{}.webp", page.index);
        let webp_path = logs_dir.join(&webp_name);

        if webp_path.exists() {
            eprintln!(
                "WebP: Náhled pro stránku {} už existuje – přeskočeno.",
                page.index
            );
            continue;
        }

        // Spustíme konverzi TIFF → WebP
        match convert_tiff_to_webp(&tiff_path, &webp_path) {
            Ok(()) => {
                generated += 1;
                eprintln!("WebP: Strana {} → {} (z TIFF)", page.index, webp_path.display());
            }
            Err(e) => {
                eprintln!("WebP: Chyba u stránky {}: {}", page.index, e);
                errors += 1;
                // Pokračujeme s další stránkou
            }
        }
    }

    if generated == 0 {
        eprintln!("WebP: Nepodařilo se vytvořit žádný náhled. Chyb: {}", errors);
    } else {
        eprintln!("WebP: Hotovo, vytvořeno {} náhledů, chyb: {}", generated, errors);
    }

    Ok(())
}

/// Převede TIFF soubor na WebP s přiměřenou kvalitou a velikostí
fn convert_tiff_to_webp(tiff_path: &Path, webp_path: &Path) -> Result<()> {
    // Načtení TIFF obrázku
    let img = image::open(tiff_path)
        .with_context(|| format!("Nelze otevřít TIFF soubor: {}", tiff_path.display()))?;

    eprintln!("DEBUG: Načten TIFF '{}': {}x{}", 
        tiff_path.file_name().unwrap_or_default().to_string_lossy(),
        img.width(), 
        img.height()
    );

    // Zmenšení obrázku pro náhled (max 1024px na delší straně)
    let max_dimension = 1024;
    let (width, height) = img.dimensions();
    
    let resized_img = if width > max_dimension || height > max_dimension {
        let ratio = width as f32 / height as f32;
        let (new_width, new_height) = if width > height {
            (max_dimension, (max_dimension as f32 / ratio) as u32)
        } else {
            ((max_dimension as f32 * ratio) as u32, max_dimension)
        };
        
        let resized = img.resize(new_width, new_height, image::imageops::FilterType::Lanczos3);
        eprintln!("DEBUG: Zmenšeno na: {}x{}", new_width, new_height);
        resized
    } else {
        eprintln!("DEBUG: Ponechána původní velikost");
        img
    };

    // Konverze na RGB(A) pro WebP encoder
    let rgb_img = resized_img.to_rgba8();
    eprintln!("DEBUG: Konvertováno na RGBA: {}x{}", rgb_img.width(), rgb_img.height());
    
    // Vytvoření WebP encoderu
    let encoder = webp::Encoder::from_rgba(&rgb_img, rgb_img.width(), rgb_img.height());
    
    // Enkódování s kvalitou 80%
    let webp_data = encoder.encode(80.0);
    
    // Získání dat jako &[u8] - TOTO JE KLÍČOVÁ OPRAVA
    let webp_bytes = webp_data.as_ref();
    
    // DEBUG: Zkontroluj velikost dat
    let data_len = webp_bytes.len();
    eprintln!("DEBUG: WebP data velikost: {} bytes", data_len);
    
    if data_len == 0 {
        return Err(anyhow!("WebP encoder vrátil prázdná data (0 bytes)"));
    }

    // Uložení WebP souboru
    std::fs::write(webp_path, webp_bytes)
        .with_context(|| format!("Nelze uložit WebP soubor: {}", webp_path.display()))?;
        
    eprintln!("DEBUG: WebP úspěšně uložen: {} bytes do {}", data_len, webp_path.display());

    // Ověření, že soubor skutečně existuje a má data
    match std::fs::metadata(webp_path) {
        Ok(metadata) => {
            let file_size = metadata.len();
            if file_size == 0 {
                return Err(anyhow!("WebP soubor byl vytvořen, ale má 0 bytes"));
            }
            eprintln!("DEBUG: Ověřeno: WebP soubor má {} bytes", file_size);
        }
        Err(e) => {
            return Err(anyhow!("Nelze ověřit vytvořený WebP soubor: {}", e));
        }
    }

    Ok(())
}

/// TEST: Alternativní implementace pro případ problémů
#[allow(dead_code)]
fn convert_tiff_to_webp_alternative(tiff_path: &Path, webp_path: &Path) -> Result<()> {
    // Alternativní implementace s explicitním vektorem
    let img = image::open(tiff_path)
        .with_context(|| format!("Nelze otevřít TIFF soubor: {}", tiff_path.display()))?;

    // Zmenšení
    let max_dimension = 1024;
    let (width, height) = img.dimensions();
    
    let resized_img = if width > max_dimension || height > max_dimension {
        let ratio = width as f32 / height as f32;
        let (new_width, new_height) = if width > height {
            (max_dimension, (max_dimension as f32 / ratio) as u32)
        } else {
            ((max_dimension as f32 * ratio) as u32, max_dimension)
        };
        
        img.resize(new_width, new_height, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    // Konverze na RGB(A)
    let rgb_img = resized_img.to_rgba8();
    
    // WebP encoding s explicitním vektorem
    let encoder = webp::Encoder::from_rgba(&rgb_img, rgb_img.width(), rgb_img.height());
    let webp_data = encoder.encode(85.0); // Vyšší kvalita
    
    // Explicitní konverze na Vec<u8>
    let webp_bytes: Vec<u8> = webp_data.to_vec();
    
    if webp_bytes.is_empty() {
        return Err(anyhow!("WebP encoder vrátil prázdný vektor"));
    }

    // Uložení
    std::fs::write(webp_path, &webp_bytes)
        .with_context(|| format!("Nelze uložit WebP soubor: {}", webp_path.display()))?;

    Ok(())
}