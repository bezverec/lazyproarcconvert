// src/html.rs
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct ManifestFile {
    path: String,
    size: u64,
    blake3: String,
}

#[derive(Debug, Deserialize)]
struct ManifestPage {
    index: String,
    
    // Podpora pro oba názvy
    #[serde(alias = "original_tiff", rename = "tiff")]
    tiff: ManifestFile,
    
    #[serde(rename = "ac_jp2")]
    ac_jp2: Option<ManifestFile>,
    
    #[serde(rename = "uc_jp2")]
    uc_jp2: Option<ManifestFile>,
    
    txt: Option<ManifestFile>,
    alto: Option<ManifestFile>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(rename = "batch_name")]
    batch_name: String,
    
    #[serde(rename = "start_index")]
    start_index: Option<u32>,
    
    #[serde(rename = "file_count")]
    file_count: Option<u32>,
    
    #[serde(rename = "input_dir")]
    input_dir: String,
    
    #[serde(rename = "output_dir")]
    output_dir: String,
    
    #[serde(rename = "logs_dir")]
    logs_dir: String,
    
    #[serde(rename = "created_at")]
    created_at: String,
    
    #[serde(rename = "generated")]
    generated: Option<String>,
    
    lang: String,
    
    #[serde(rename = "alto_version")]
    alto_version: String,
    
    pages: Vec<ManifestPage>,
}

pub fn write_html_report(logs_dir: &Path) -> Result<()> {
    // Debug výpis pro diagnostiku
    println!("DEBUG: Generuji HTML report z adresáře: {:?}", logs_dir);
    
    let manifest_path = logs_dir.join("manifest.json");
    println!("DEBUG: Hledám manifest: {:?}", manifest_path);
    
    // Zkontrolovat existenci manifestu
    if !manifest_path.exists() {
        return Err(anyhow::anyhow!("Manifest.json neexistuje v: {:?}", manifest_path));
    }
    
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Nelze načíst manifest.json z {:?}", manifest_path))?;
    
    println!("DEBUG: Manifest načten, velikost: {} znaků", manifest_text.len());
    
    // Debug: zobrazit začátek manifestu
    let preview_len = manifest_text.len().min(200);
    println!("DEBUG: Náhled manifestu (prvních {} znaků): {}", preview_len, &manifest_text[..preview_len]);
    
    // Parsovat manifest - UPRAVENÁ ČÁST
    let manifest: Manifest = match serde_json::from_str::<Manifest>(&manifest_text) {
        Ok(m) => {
            println!("DEBUG: Manifest úspěšně parsován");
            println!("DEBUG: Batch: {}, stránek: {}", m.batch_name, m.pages.len());
            m
        }
        Err(e) => {
            eprintln!("DEBUG: CHYBA PARSOVÁNÍ MANIFESTU: {}", e);
            eprintln!("DEBUG: Detaily: {:?}", e);
            return Err(e.into());
        }
    };
    
    let batch_name = manifest.batch_name.clone();
    let lang = manifest.lang.clone();
    let alto_version = manifest.alto_version.clone();
    
    // Pro ladění: zobrazit informace o stránkách
    for (i, page) in manifest.pages.iter().enumerate() {
        println!("DEBUG: Stránka {}: index={}, txt={:?}, alto={:?}", 
            i, page.index, 
            page.txt.as_ref().map(|t| t.path.as_str()),
            page.alto.as_ref().map(|a| a.path.as_str()));
    }
    
    let rel_prefix = format!("../{}", batch_name);

    let pages_json: Vec<_> = manifest
        .pages
        .iter()
        .map(|p| {
            // Zůstává stejné - extrakce názvů souborů z cest
            let txt_name = Path::new(&p.txt.as_ref().map(|t| &t.path).unwrap_or(&String::new()))
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let alto_name = Path::new(&p.alto.as_ref().map(|a| &a.path).unwrap_or(&String::new()))
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            let txt_url = if txt_name.is_empty() {
                None
            } else {
                Some(format!("{}/{}", rel_prefix, txt_name).replace('\\', "/"))
            };
            let alto_url = if alto_name.is_empty() {
                None
            } else {
                Some(format!("{}/{}", rel_prefix, alto_name).replace('\\', "/"))
            };

            let image_url = format!("page_{}.webp", p.index);

            json!({
                "index": p.index,
                "txtUrl": txt_url,
                "altoUrl": alto_url,
                "imageUrl": image_url,
                "txtName": txt_name,
                "altoName": alto_name,
            })
        })
        .collect();

    let client_manifest = json!({
        "batchName": batch_name,
        "createdAt": manifest.created_at,
        "lang": lang,
        "altoVersion": alto_version,
        "pages": pages_json,
    });

    let manifest_js = client_manifest.to_string();

    let mut html = String::new();

    html.push_str(
        r#"<!DOCTYPE html>
<html lang="cs">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>LazyProArcConvert – ALTO náhled & editor</title>
  <style>
    :root {
      color-scheme: dark;
      --bg: #0b0c10;
      --bg-panel: #161824;
      --bg-panel-alt: #1e2130;
      --border-soft: #2a2f40;
      --accent: #4fc3f7;
      --accent-soft: rgba(79, 195, 247, 0.08);
      --text-main: #f5f7ff;
      --text-muted: #9ca3af;
      --danger: #ff6b6b;
      --success: #4caf50;
      --scroll: #303550;
    }

    * {
      box-sizing: border-box;
    }

    body {
      margin: 0;
      padding: 0;
      background: radial-gradient(circle at top, #202542 0, #050611 55%);
      color: var(--text-main);
      font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      height: 100vh;
      display: flex;
      flex-direction: column;
    }

    header {
      padding: 10px 14px;
      border-bottom: 1px solid var(--border-soft);
      background: linear-gradient(90deg, #12141f, #191c2b);
      display: flex;
      align-items: baseline;
      gap: 12px;
    }

    header h1 {
      font-size: 18px;
      margin: 0;
    }

    header .meta {
      font-size: 13px;
      color: var(--text-muted);
    }

    main {
      flex: 1;
      display: grid;
      grid-template-columns: 220px minmax(300px, 1.6fr) minmax(260px, 0.9fr);
      gap: 8px;
      padding: 8px;
      overflow: hidden;
    }

    .panel {
      background: var(--bg-panel);
      border-radius: 10px;
      border: 1px solid var(--border-soft);
      padding: 8px;
      display: flex;
      flex-direction: column;
      min-height: 0;
    }

    .panel.main {
      background: radial-gradient(circle at top left, rgba(79,195,247,0.08) 0, #0b0c10 45%);
    }

    .panel h2 {
      margin: 0 0 6px 0;
      font-size: 14px;
      letter-spacing: 0.03em;
      text-transform: uppercase;
      color: var(--text-muted);
      display: flex;
      justify-content: space-between;
      align-items: center;
    }

    .panel h2 .controls {
      display: flex;
      gap: 4px;
    }

    /* Seznam stran */
    #pageList {
      flex: 1;
      overflow-y: auto;
      padding-right: 4px;
    }

    #pageList button {
      width: 100%;
      text-align: left;
      padding: 4px 6px;
      margin-bottom: 3px;
      border-radius: 6px;
      border: 1px solid transparent;
      background: transparent;
      color: var(--text-main);
      font: inherit;
      cursor: pointer;
      display: flex;
      justify-content: space-between;
      align-items: center;
    }

    #pageList button span.index {
      font-variant-numeric: tabular-nums;
    }

    #pageList button span.badge {
      font-size: 11px;
      color: var(--text-muted);
    }

    #pageList button:hover {
      background: var(--accent-soft);
      border-color: rgba(79,195,247,0.4);
    }

    #pageList button.active {
      background: rgba(79,195,247,0.2);
      border-color: var(--accent);
    }

    /* hlavní náhled */
    .image-container {
      position: relative;
      flex: 1;
      display: flex;
      justify-content: center;
      align-items: center;
      background: radial-gradient(circle at center, #1a1d2c 0, #050611 60%);
      border-radius: 8px;
      overflow: hidden;
      min-height: 0;
    }

    .zoom-wrapper {
      position: absolute;
      top: 0;
      left: 0;
      width: 100%;
      height: 100%;
      overflow: hidden;
      transform-origin: 0 0;
      will-change: transform;
    }

    .image-canvas-container {
      position: relative;
      width: 100%;
      height: 100%;
    }

    #pageImage {
      position: absolute;
      top: 0;
      left: 0;
      width: 100%;
      height: 100%;
      object-fit: contain;
      pointer-events: none;
    }

    #altoCanvas {
      position: absolute;
      top: 0;
      left: 0;
      width: 100%;
      height: 100%;
      pointer-events: auto;
      cursor: default;
    }

    .overlay-note {
      position: absolute;
      bottom: 8px;
      right: 10px;
      font-size: 11px;
      color: var(--text-muted);
      background: rgba(5, 6, 17, 0.85);
      padding: 4px 6px;
      border-radius: 999px;
      border: 1px solid rgba(79,195,247,0.3);
      z-index: 10;
    }

    .layer-toggles {
      margin-top: 6px;
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
      font-size: 12px;
    }

    .layer-toggles label {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      padding: 2px 6px;
      border-radius: 999px;
      background: #11131f;
      border: 1px solid var(--border-soft);
      cursor: pointer;
    }

    .layer-printspace {
      border-color: rgba(0,255,0,0.5);
    }
    .layer-blocks {
      border-color: rgba(0,128,255,0.7);
    }
    .layer-lines {
      border-color: rgba(255,165,0,0.7);
    }
    .layer-words {
      border-color: rgba(255,0,0,0.7);
    }

    .layer-dot {
      width: 10px;
      height: 10px;
      border-radius: 999px;
    }
    .layer-printspace .layer-dot {
      background: rgba(0,255,0,0.7);
    }
    .layer-blocks .layer-dot {
      background: rgba(0,128,255,0.9);
    }
    .layer-lines .layer-dot {
      background: rgba(255,165,0,0.9);
    }
    .layer-words .layer-dot {
      background: rgba(255,0,0,0.9);
    }

    .status-bar {
      margin-top: 6px;
      font-size: 12px;
      color: var(--text-muted);
      display: flex;
      justify-content: space-between;
      align-items: center;
    }

    /* Info panel */
    .info-grid {
      font-size: 12px;
      border-radius: 6px;
      border: 1px solid var(--border-soft);
      background: var(--bg-panel-alt);
      padding: 6px;
      margin-bottom: 6px;
    }

    .info-grid .row {
      display: grid;
      grid-template-columns: 80px 1fr;
      gap: 4px;
      margin-bottom: 3px;
    }

    .info-grid .row .label {
      color: var(--text-muted);
    }

    /* OCR text viewer */
    #ocrText {
      flex: 1;
      margin-top: 6px;
      padding: 6px;
      border-radius: 6px;
      border: 1px solid var(--border-soft);
      background: #0d0f18;
      font-size: 12px;
      white-space: pre-wrap;
      overflow-y: auto;
      font-family: 'Consolas', 'Monaco', monospace;
      line-height: 1.4;
      resize: none;
      outline: none;
      color: var(--text-main);
      min-height: 150px;
      cursor: text;
      user-select: text;
    }

    #ocrText:focus {
      border-color: var(--accent);
      box-shadow: 0 0 0 1px rgba(79, 195, 247, 0.2);
    }

    .ocr-highlight {
      background-color: yellow !important;
      color: black !important;
      padding: 0 2px;
      border-radius: 2px;
    }

    /* ALTO editor */
    .alto-editor {
      flex: 1;
      margin-top: 6px;
      display: flex;
      flex-direction: column;
      gap: 6px;
      min-height: 0;
      overflow: hidden;
    }

    .alto-tools {
      display: flex;
      gap: 4px;
      margin-bottom: 6px;
      padding: 4px;
      background: var(--bg-panel-alt);
      border-radius: 4px;
      border: 1px solid var(--border-soft);
      flex-wrap: wrap;
    }

    .alto-tool-btn {
      padding: 4px 8px;
      border-radius: 4px;
      border: 1px solid var(--border-soft);
      background: #11131f;
      color: var(--text-main);
      font-size: 11px;
      cursor: pointer;
      display: flex;
      align-items: center;
      gap: 4px;
      white-space: nowrap;
    }

    .alto-tool-btn:hover {
      background: var(--accent-soft);
      border-color: var(--accent);
    }

    .alto-tool-btn.active {
      background: var(--accent);
      border-color: var(--accent);
      color: #0b0c10;
    }

    .btn {
      padding: 4px 8px;
      border-radius: 4px;
      border: 1px solid var(--border-soft);
      background: #11131f;
      color: var(--text-main);
      font-size: 11px;
      cursor: pointer;
      transition: all 0.2s;
      display: flex;
      align-items: center;
      gap: 4px;
      white-space: nowrap;
    }

    .btn:hover {
      background: var(--accent-soft);
      border-color: var(--accent);
    }

    .btn.primary {
      background: var(--accent);
      border-color: var(--accent);
      color: #0b0c10;
    }

    .btn.primary:hover {
      background: rgba(79,195,247,0.8);
    }

    .btn.danger {
      background: var(--danger);
      border-color: var(--danger);
      color: #0b0c10;
    }

    .btn:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }

    .alto-xml-editor {
      flex: 1;
      border: 1px solid var(--border-soft);
      border-radius: 6px;
      background: #0d0f18;
      overflow: hidden;
      position: relative;
      display: flex;
      flex-direction: column;
      min-height: 0;
    }

    #altoXmlText {
      flex: 1;
      width: 100%;
      min-height: 200px;
      padding: 8px;
      background: transparent;
      color: var(--text-main);
      font-family: 'Consolas', 'Monaco', monospace;
      font-size: 11px;
      line-height: 1.3;
      border: none;
      resize: none;
      outline: none;
      white-space: pre;
      overflow: auto;
    }

    .alto-element-list {
      flex: 1;
      border: 1px solid var(--border-soft);
      border-radius: 6px;
      background: #0d0f18;
      overflow-y: auto;
      padding: 4px;
      min-height: 0;
    }

    .alto-element-item {
      padding: 6px 8px;
      margin-bottom: 4px;
      border-radius: 4px;
      border: 1px solid transparent;
      background: #11131f;
      font-size: 11px;
      cursor: pointer;
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 8px;
    }

    .alto-element-item:hover {
      background: var(--accent-soft);
      border-color: rgba(79,195,247,0.4);
    }

    .alto-element-item.active {
      background: rgba(79,195,247,0.2);
      border-color: var(--accent);
    }

    .alto-element-item.selected {
      background: rgba(79,195,247,0.3);
      border-color: var(--accent);
      box-shadow: 0 0 0 1px var(--accent);
    }

    .element-content {
      flex: 1;
      min-width: 0;
      overflow: hidden;
    }

    .element-content strong {
      display: block;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      margin-bottom: 2px;
    }

    .element-type {
      font-size: 10px;
      color: var(--text-muted);
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }

    .element-type.word {
      color: rgba(255,0,0,0.7);
    }
    .element-type.line {
      color: rgba(255,165,0,0.7);
    }
    .element-type.block {
      color: rgba(0,128,255,0.7);
    }

    .element-actions {
      display: flex;
      gap: 2px;
    }

    .element-action-btn {
      padding: 2px 4px;
      border-radius: 2px;
      border: 1px solid transparent;
      background: transparent;
      color: var(--text-muted);
      font-size: 10px;
      cursor: pointer;
    }

    .element-action-btn:hover {
      background: var(--accent-soft);
      border-color: rgba(79,195,247,0.4);
      color: var(--text-main);
    }

    .element-action-btn.delete:hover {
      background: rgba(255,107,107,0.2);
      border-color: var(--danger);
      color: var(--danger);
    }

    .element-action-btn.edit:hover {
      background: rgba(79,195,247,0.2);
      border-color: var(--accent);
      color: var(--accent);
    }

    .element-coords {
      font-size: 10px;
      color: var(--text-muted);
      text-align: right;
      white-space: nowrap;
    }

    /* Element editor */
    .element-editor {
      border: 1px solid var(--border-soft);
      border-radius: 6px;
      background: #0d0f18;
      padding: 8px;
      margin-top: 6px;
    }

    .element-editor h4 {
      margin: 0 0 8px 0;
      font-size: 12px;
      color: var(--text-main);
    }

    .coord-inputs {
      display: grid;
      grid-template-columns: repeat(4, 1fr);
      gap: 6px;
      margin-bottom: 8px;
    }

    .coord-input {
      display: flex;
      flex-direction: column;
      gap: 2px;
    }

    .coord-input label {
      font-size: 10px;
      color: var(--text-muted);
    }

    .coord-input input {
      padding: 4px 6px;
      border-radius: 4px;
      border: 1px solid var(--border-soft);
      background: #11131f;
      color: var(--text-main);
      font-size: 11px;
      font-family: monospace;
    }

    .text-input {
      display: flex;
      flex-direction: column;
      gap: 2px;
    }

    .text-input label {
      font-size: 10px;
      color: var(--text-muted);
    }

    .text-input input {
      padding: 4px 6px;
      border-radius: 4px;
      border: 1px solid var(--border-soft);
      background: #11131f;
      color: var(--text-main);
      font-size: 11px;
      font-family: 'Consolas', 'Monaco', monospace;
    }

    .element-editor-buttons {
      display: flex;
      gap: 6px;
      margin-top: 8px;
    }

    /* Text editor inline */
    .element-text-editor {
      margin-top: 4px;
      display: flex;
      gap: 4px;
      align-items: center;
    }

    .element-text-input {
      flex: 1;
      padding: 4px 6px;
      border-radius: 4px;
      border: 1px solid var(--border-soft);
      background: #0d0f18;
      color: var(--text-main);
      font-size: 11px;
      font-family: 'Consolas', 'Monaco', monospace;
    }

    .element-text-input:focus {
      border-color: var(--accent);
      outline: none;
    }

    /* Zoom controls */
    .zoom-controls {
      position: absolute;
      top: 8px;
      left: 8px;
      display: flex;
      gap: 4px;
      z-index: 20;
    }

    .zoom-btn {
      width: 28px;
      height: 28px;
      border-radius: 4px;
      background: rgba(5, 6, 17, 0.85);
      border: 1px solid var(--border-soft);
      color: var(--text-main);
      font-size: 14px;
      cursor: pointer;
      display: flex;
      align-items: center;
      justify-content: center;
      transition: all 0.2s;
    }

    .zoom-btn:hover {
      background: rgba(79,195,247,0.1);
      border-color: var(--accent);
    }

    .zoom-display {
      position: absolute;
      top: 8px;
      right: 8px;
      background: rgba(5, 6, 17, 0.85);
      border: 1px solid rgba(79,195,247,0.3);
      border-radius: 999px;
      padding: 2px 8px;
      font-size: 11px;
      color: var(--text-muted);
      z-index: 20;
    }

    /* Edit mode indicator */
    .edit-mode {
      position: absolute;
      top: 8px;
      left: 50%;
      transform: translateX(-50%);
      background: rgba(255, 193, 7, 0.9);
      color: #0b0c10;
      padding: 2px 8px;
      border-radius: 999px;
      font-size: 11px;
      font-weight: bold;
      z-index: 20;
    }

    /* Cursor styles */
    .cursor-move { cursor: move !important; }
    .cursor-resize-n { cursor: n-resize !important; }
    .cursor-resize-s { cursor: s-resize !important; }
    .cursor-resize-e { cursor: e-resize !important; }
    .cursor-resize-w { cursor: w-resize !important; }
    .cursor-resize-ne { cursor: ne-resize !important; }
    .cursor-resize-nw { cursor: nw-resize !important; }
    .cursor-resize-se { cursor: se-resize !important; }
    .cursor-resize-sw { cursor: sw-resize !important; }
    .cursor-crosshair { cursor: crosshair !important; }

    /* scrollbary */
    *::-webkit-scrollbar {
      width: 8px;
      height: 8px;
    }
    *::-webkit-scrollbar-track {
      background: transparent;
    }
    *::-webkit-scrollbar-thumb {
      background: var(--scroll);
      border-radius: 999px;
    }

    .loader {
      display: inline-block;
      width: 12px;
      height: 12px;
      border: 2px solid var(--text-muted);
      border-radius: 50%;
      border-top-color: var(--accent);
      animation: spin 1s ease-in-out infinite;
    }

    @keyframes spin {
      to { transform: rotate(360deg); }
    }

    /* Modal */
    .modal {
      position: fixed;
      top: 0;
      left: 0;
      width: 100%;
      height: 100%;
      background: rgba(0, 0, 0, 0.7);
      display: flex;
      justify-content: center;
      align-items: center;
      z-index: 2000;
      opacity: 0;
      visibility: hidden;
      transition: all 0.3s;
    }

    .modal.active {
      opacity: 1;
      visibility: visible;
    }

    .modal-content {
      background: var(--bg-panel);
      border-radius: 8px;
      border: 1px solid var(--border-soft);
      padding: 16px;
      min-width: 300px;
      max-width: 500px;
      max-height: 80vh;
      overflow-y: auto;
    }

    .modal h3 {
      margin: 0 0 12px 0;
      color: var(--text-main);
    }

    .modal p {
      margin: 0 0 16px 0;
      color: var(--text-muted);
      font-size: 13px;
      line-height: 1.4;
    }

    .modal-buttons {
      display: flex;
      justify-content: flex-end;
      gap: 8px;
      margin-top: 16px;
    }

    /* Notification */
    .notification {
      position: fixed;
      bottom: 16px;
      right: 16px;
      padding: 8px 16px;
      border-radius: 6px;
      background: var(--bg-panel);
      border: 1px solid var(--border-soft);
      color: var(--text-main);
      font-size: 13px;
      z-index: 2000;
      opacity: 0;
      transform: translateY(20px);
      transition: all 0.3s;
    }

    .notification.show {
      opacity: 1;
      transform: translateY(0);
    }

    .notification.info {
      border-color: var(--accent);
      background: rgba(79, 195, 247, 0.1);
    }
  </style>
</head>
<body>
<header>
  <h1>LazyProArcConvert – ALTO náhled & editor</h1>
  <div class="meta">
    <span id="metaBatch"></span> ·
    <span id="metaLang"></span> ·
    <span id="metaAlto"></span>
  </div>
</header>
<main>
  <section class="panel">
    <h2>Stránky</h2>
    <div id="pageList"></div>
  </section>

  <section class="panel main">
    <h2>
      Náhled
      <div class="controls">
        <button class="btn" id="editAltoBtn" title="Editovat ALTO">
          <span>✏️</span> Edit
        </button>
      </div>
    </h2>
    <div class="image-container">
      <div class="zoom-controls">
        <button class="zoom-btn" id="zoomIn" title="Zoom in">+</button>
        <button class="zoom-btn" id="zoomOut" title="Zoom out">-</button>
        <button class="zoom-btn" id="zoomReset" title="Reset zoom">1:1</button>
      </div>
      <div class="zoom-display" id="zoomDisplay">100%</div>
      <div id="editModeIndicator" class="edit-mode" style="display: none">EDIT MODE</div>
      <div class="zoom-wrapper" id="zoomWrapper">
        <div class="image-canvas-container" id="imageCanvasContainer">
          <img id="pageImage" alt="Náhled stránky">
          <canvas id="altoCanvas"></canvas>
        </div>
      </div>
      <div class="overlay-note">Klikni na element pro zvýraznění textu • Drag to pan • Wheel to zoom</div>
    </div>
    <div class="layer-toggles">
      <label class="layer-printspace">
        <span class="layer-dot"></span>
        <input type="checkbox" id="layerPrintspace" checked>
        PrintSpace
      </label>
      <label class="layer-blocks">
        <span class="layer-dot"></span>
        <input type="checkbox" id="layerBlocks" checked>
        Bloky
      </label>
      <label class="layer-lines">
        <span class="layer-dot"></span>
        <input type="checkbox" id="layerLines" checked>
        Řádky
      </label>
      <label class="layer-words">
        <span class="layer-dot"></span>
        <input type="checkbox" id="layerWords" checked>
        Slova
      </label>
    </div>
    <div id="statusBar" class="status-bar">
      <span><span class="loader"></span> Načítám…</span>
      <span id="saveStatus"></span>
    </div>
  </section>

  <section class="panel">
    <h2>
      Text & ALTO editor
      <div class="controls">
        <button class="btn" id="saveTxtBtn" title="Uložit TXT změny">
          <span>💾</span> TXT
        </button>
      </div>
    </h2>
    <div class="info-grid">
      <div class="row">
        <div class="label">Index</div>
        <div id="infoIndex"></div>
      </div>
      <div class="row">
        <div class="label">Elementů</div>
        <div id="infoElements">-</div>
      </div>
      <div class="row">
        <div class="label">Rozměry</div>
        <div id="infoDimensions">-</div>
      </div>
      <div class="row">
        <div class="label">Zoom</div>
        <div id="infoZoom">100%</div>
      </div>
    </div>
    
    <!-- TXT editor -->
    <textarea id="ocrText" placeholder="OCR text se načte automaticky..."></textarea>
    
    <!-- ALTO editor -->
    <div class="alto-editor" id="altoEditor" style="display: none;">
      <div class="alto-tools" id="altoTools">
        <button class="alto-tool-btn active" id="toolSelect" title="Výběr" data-tool="select">
          <span>✥</span> Výběr
        </button>
        <button class="alto-tool-btn" id="toolCreateWord" title="Vytvořit slovo" data-tool="createWord">
          <span>W</span> Slovo
        </button>
        <button class="alto-tool-btn" id="toolCreateLine" title="Vytvořit řádek" data-tool="createLine">
          <span>L</span> Řádek
        </button>
        <button class="alto-tool-btn" id="toolCreateBlock" title="Vytvořit blok" data-tool="createBlock">
          <span>B</span> Blok
        </button>
        <button class="alto-tool-btn" id="toolDelete" title="Smazat" data-tool="delete">
          <span>🗑</span> Smazat
        </button>
      </div>
      
      <div class="alto-controls">
        <button class="btn" id="showXmlBtn">XML</button>
        <button class="btn" id="showElementsBtn">Elementy</button>
        <button class="btn primary" id="saveAltoBtn">💾 Uložit ALTO</button>
        <button class="btn danger" id="cancelEditBtn">✕ Zrušit</button>
      </div>
      
      <div class="alto-xml-editor" id="altoXmlEditor" style="display: none;">
        <textarea id="altoXmlText" spellcheck="false"></textarea>
      </div>
      <div class="alto-element-list" id="altoElementList"></div>
    </div>
  </section>
</main>

<!-- Modals -->
<div class="modal" id="saveModal">
  <div class="modal-content">
    <h3>Uložit změny</h3>
    <p id="saveModalText"></p>
    <div class="modal-buttons">
      <button class="btn" id="saveModalCancel">Zrušit</button>
      <button class="btn primary" id="saveModalConfirm">Uložit</button>
    </div>
  </div>
</div>

<div class="modal" id="deleteModal">
  <div class="modal-content">
    <h3>Smazat element</h3>
    <p id="deleteModalText"></p>
    <div class="modal-buttons">
      <button class="btn" id="deleteModalCancel">Zrušit</button>
      <button class="btn danger" id="deleteModalConfirm">Smazat</button>
    </div>
  </div>
</div>

<div class="modal" id="textEditModal">
  <div class="modal-content">
    <h3>Editovat text</h3>
    <input type="text" id="textEditInput" class="element-text-input" style="width: 100%; margin: 12px 0;">
    <div class="modal-buttons">
      <button class="btn" id="textEditCancel">Zrušit</button>
      <button class="btn primary" id="textEditConfirm">Uložit</button>
    </div>
  </div>
</div>

<div class="notification" id="notification"></div>

<script>"#,
    );

    html.push_str("const MANIFEST = ");
    html.push_str(&manifest_js);
    html.push_str(";\n");

    html.push_str(
        r#"
(function() {
  'use strict';

  // === STATE ===
  let currentPageIndex = 0;
  let currentAltoData = null;
  let currentAltoXml = '';
  let fullOcrText = '';
  let scale = 1.0;
  let offsetX = 0;
  let offsetY = 0;
  let isDragging = false;
  let lastMouseX = 0;
  let lastMouseY = 0;
  let imageNaturalWidth = 0;
  let imageNaturalHeight = 0;
  let containerWidth = 0;
  let containerHeight = 0;
  let imageScale = 1.0;
  let imageOffsetX = 0;
  let imageOffsetY = 0;
  let isEditingAlto = false;
  let txtModified = false;
  let altoModified = false;
  let currentSelectedElement = null;
  let currentHoveredElement = null;
  let editingElement = null;
  let altoDisplayMode = 'elements';
  
  // Edit mode
  let currentTool = 'select';
  let isDraggingElement = false;
  let dragElement = null;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragStartAltoX = 0;
  let dragStartAltoY = 0;
  let dragStartWidth = 0;
  let dragStartHeight = 0;
  let resizeHandle = null;
  let isCreatingElement = false;
  let createStartX = 0;
  let createStartY = 0;
  let createCurrentX = 0;
  let createCurrentY = 0;
  let highlightTimer = null;

  // Text edit state
  let editingWordElement = null;
  let originalFullOcrText = '';

  // Element mapping for quick lookup
  let elementIdMap = new Map();
  let selectedElementId = null; // Track which element is selected in the panel

  // === DOM ELEMENTS ===
  const pageListEl = document.getElementById('pageList');
  const imgEl = document.getElementById('pageImage');
  const canvas = document.getElementById('altoCanvas');
  const ctx = canvas?.getContext('2d');
  const ocrTextEl = document.getElementById('ocrText');
  const editModeIndicator = document.getElementById('editModeIndicator');
  const zoomDisplayEl = document.getElementById('zoomDisplay');
  const infoIndexEl = document.getElementById('infoIndex');
  const infoElementsEl = document.getElementById('infoElements');
  const infoDimensionsEl = document.getElementById('infoDimensions');
  const infoZoomEl = document.getElementById('infoZoom');
  const statusEl = document.getElementById('statusBar');
  const saveStatusEl = document.getElementById('saveStatus');
  
  // Edit controls
  const editAltoBtn = document.getElementById('editAltoBtn');
  const altoEditor = document.getElementById('altoEditor');
  const altoXmlEditor = document.getElementById('altoXmlEditor');
  const altoElementList = document.getElementById('altoElementList');
  const altoXmlText = document.getElementById('altoXmlText');
  const showXmlBtn = document.getElementById('showXmlBtn');
  const showElementsBtn = document.getElementById('showElementsBtn');
  const saveAltoBtn = document.getElementById('saveAltoBtn');
  const cancelEditBtn = document.getElementById('cancelEditBtn');
  const saveTxtBtn = document.getElementById('saveTxtBtn');
  
  // Tool buttons
  const altoTools = document.getElementById('altoTools');
  const toolSelect = document.getElementById('toolSelect');
  const toolCreateWord = document.getElementById('toolCreateWord');
  const toolCreateLine = document.getElementById('toolCreateLine');
  const toolCreateBlock = document.getElementById('toolCreateBlock');
  const toolDelete = document.getElementById('toolDelete');
  
  // Modals
  const saveModal = document.getElementById('saveModal');
  const saveModalText = document.getElementById('saveModalText');
  const saveModalCancel = document.getElementById('saveModalCancel');
  const saveModalConfirm = document.getElementById('saveModalConfirm');
  const deleteModal = document.getElementById('deleteModal');
  const deleteModalText = document.getElementById('deleteModalText');
  const deleteModalCancel = document.getElementById('deleteModalCancel');
  const deleteModalConfirm = document.getElementById('deleteModalConfirm');
  const textEditModal = document.getElementById('textEditModal');
  const textEditInput = document.getElementById('textEditInput');
  const textEditCancel = document.getElementById('textEditCancel');
  const textEditConfirm = document.getElementById('textEditConfirm');
  const notification = document.getElementById('notification');

  // Layer toggles
  const layerPrintspace = document.getElementById('layerPrintspace');
  const layerBlocks = document.getElementById('layerBlocks');
  const layerLines = document.getElementById('layerLines');
  const layerWords = document.getElementById('layerWords');

  // Zoom controls
  const zoomInBtn = document.getElementById('zoomIn');
  const zoomOutBtn = document.getElementById('zoomOut');
  const zoomResetBtn = document.getElementById('zoomReset');
  const zoomWrapper = document.getElementById('zoomWrapper');

  // === UTILITY FUNCTIONS ===
  function setStatus(msg, isError = false) {
    if (!statusEl) return;
    const loader = statusEl.querySelector('.loader');
    if (loader) loader.style.display = 'inline-block';
    statusEl.querySelector('span').textContent = msg;
    statusEl.style.color = isError ? 'var(--danger)' : 'var(--text-muted)';
  }

  function hideLoader() {
    const loader = statusEl.querySelector('.loader');
    if (loader) loader.style.display = 'none';
  }

  function showNotification(message, duration = 3000) {
    notification.textContent = message;
    notification.className = 'notification info';
    notification.classList.add('show');
    
    setTimeout(() => {
      notification.classList.remove('show');
    }, duration);
  }

  function showModal(modal) {
    modal.classList.add('active');
  }

  function hideModal(modal) {
    modal.classList.remove('active');
  }

  function updateZoomDisplay() {
    const percentage = Math.round(scale * 100);
    if (zoomDisplayEl) zoomDisplayEl.textContent = `${percentage}%`;
    if (infoZoomEl) infoZoomEl.textContent = `${percentage}%`;
  }

  function calculateImageFit() {
    if (!imgEl || imageNaturalWidth === 0 || imageNaturalHeight === 0) return;
    
    const container = document.getElementById('imageCanvasContainer');
    if (!container) return;
    
    containerWidth = container.clientWidth;
    containerHeight = container.clientHeight;
    
    const scaleX = containerWidth / imageNaturalWidth;
    const scaleY = containerHeight / imageNaturalHeight;
    imageScale = Math.min(scaleX, scaleY);
    
    imageOffsetX = (containerWidth - imageNaturalWidth * imageScale) / 2;
    imageOffsetY = (containerHeight - imageNaturalHeight * imageScale) / 2;
    
    if (infoDimensionsEl) {
      infoDimensionsEl.textContent = `${imageNaturalWidth}×${imageNaturalHeight}px`;
    }
  }

  function applyTransform() {
    zoomWrapper.style.transform = `translate(${offsetX}px, ${offsetY}px) scale(${scale})`;
    updateZoomDisplay();
    drawAltoLayers();
  }

  function zoomToPoint(factor, clientX, clientY) {
    const rect = zoomWrapper.getBoundingClientRect();
    const mouseX = clientX - rect.left;
    const mouseY = clientY - rect.top;
    
    const oldX = (mouseX - offsetX) / scale;
    const oldY = (mouseY - offsetY) / scale;
    
    const newScale = Math.max(0.1, Math.min(10, scale * factor));
    
    offsetX = mouseX - oldX * newScale;
    offsetY = mouseY - oldY * newScale;
    scale = newScale;
    
    applyTransform();
  }

  // === COORDINATE CONVERSIONS ===
  function canvasToAltoCoordinates(canvasX, canvasY) {
    if (!currentAltoData || imageNaturalWidth === 0 || imageNaturalHeight === 0) {
      return { x: 0, y: 0 };
    }
    
    const relativeX = canvasX - imageOffsetX;
    const relativeY = canvasY - imageOffsetY;
    
    if (relativeX < 0 || relativeY < 0) return { x: 0, y: 0 };
    
    const imageX = relativeX / imageScale;
    const imageY = relativeY / imageScale;
    
    const altoWidth = currentAltoData.pageWidth || imageNaturalWidth;
    const altoHeight = currentAltoData.pageHeight || imageNaturalHeight;
    
    const altoX = (imageX / imageNaturalWidth) * altoWidth;
    const altoY = (imageY / imageNaturalHeight) * altoHeight;
    
    return { 
      x: Math.max(0, Math.min(altoWidth, altoX)),
      y: Math.max(0, Math.min(altoHeight, altoY))
    };
  }

  function altoToCanvasCoordinates(altoX, altoY, altoW, altoH) {
    if (!currentAltoData || imageNaturalWidth === 0 || imageNaturalHeight === 0) {
      return { x: 0, y: 0, w: 0, h: 0 };
    }
    
    const altoWidth = currentAltoData.pageWidth || imageNaturalWidth;
    const altoHeight = currentAltoData.pageHeight || imageNaturalHeight;
    
    const imageX = (altoX / altoWidth) * imageNaturalWidth;
    const imageY = (altoY / altoHeight) * imageNaturalHeight;
    const imageW = (altoW / altoWidth) * imageNaturalWidth;
    const imageH = (altoH / altoHeight) * imageNaturalHeight;
    
    const x = (imageX * imageScale) + imageOffsetX;
    const y = (imageY * imageScale) + imageOffsetY;
    const w = imageW * imageScale;
    const h = imageH * imageScale;
    
    return { x, y, w, h };
  }

  // === PAGE MANAGEMENT ===
  function createPageList() {
    if (!pageListEl) return;
    
    const pages = MANIFEST.pages || [];
    if (pages.length === 0) {
      setStatus("Manifest neobsahuje žádné stránky.", true);
      return;
    }
    
    pageListEl.innerHTML = '';
    
    pages.forEach((page, idx) => {
      const btn = document.createElement('button');
      btn.innerHTML = `
        <span class="index">${page.index || idx + 1}</span>
        <span class="badge">ALTO</span>
      `;
      btn.dataset.idx = idx;
      btn.addEventListener('click', () => {
        if (txtModified || altoModified) {
          showSaveConfirmation(() => selectPage(idx));
        } else {
          selectPage(idx);
        }
      });
      pageListEl.appendChild(btn);
    });
  }

  function highlightActivePage() {
    const buttons = pageListEl.querySelectorAll('button');
    buttons.forEach((btn, idx) => {
      btn.classList.toggle('active', idx === currentPageIndex);
    });
  }

  async function selectPage(idx) {
    const pages = MANIFEST.pages || [];
    if (idx < 0 || idx >= pages.length) return;
    
    currentPageIndex = idx;
    const page = pages[idx];
    
    exitEditModes();
    clearHighlight();
    
    highlightActivePage();
    setStatus(`Načítám stránku ${page.index}...`);
    
    // Reset view
    scale = 1.0;
    offsetX = 0;
    offsetY = 0;
    
    // Update info
    if (infoIndexEl) infoIndexEl.textContent = page.index || '-';
    if (infoElementsEl) infoElementsEl.textContent = '-';
    
    // Load data
    await Promise.all([
      loadImage(page),
      loadTxt(page),
      loadAlto(page)
    ]);
    
    applyTransform();
    hideLoader();
  }

  function loadImage(page) {
    return new Promise(resolve => {
      if (!imgEl) {
        resolve();
        return;
      }
      
      imgEl.onload = function() {
        imageNaturalWidth = imgEl.naturalWidth;
        imageNaturalHeight = imgEl.naturalHeight;
        
        setTimeout(() => {
          calculateImageFit();
          if (canvas) {
            canvas.width = containerWidth;
            canvas.height = containerHeight;
          }
          resolve();
        }, 50);
      };
      
      imgEl.onerror = function() {
        console.error('Chyba načítání obrázku:', page.imageUrl);
        resolve();
      };
      
      imgEl.src = page.imageUrl || '';
    });
  }

  function loadTxt(page) {
    return new Promise(resolve => {
      if (!page.txtUrl || !ocrTextEl) {
        ocrTextEl.value = '';
        fullOcrText = '';
        resolve();
        return;
      }
      
      fetch(page.txtUrl)
        .then(resp => resp.ok ? resp.text() : Promise.reject(`HTTP ${resp.status}`))
        .then(text => {
          fullOcrText = text || '';
          originalFullOcrText = fullOcrText;
          ocrTextEl.value = fullOcrText;
          txtModified = false;
          updateSaveButtons();
          resolve();
        })
        .catch(err => {
          console.error('Chyba načítání TXT:', err);
          ocrTextEl.value = '';
          fullOcrText = '';
          resolve();
        });
    });
  }

  async function loadAlto(page) {
    if (!page.altoUrl || !canvas || !ctx) {
      currentAltoData = null;
      return;
    }
    
    try {
      const response = await fetch(page.altoUrl);
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      
      currentAltoXml = await response.text();
      
      // Parse XML
      const parser = new DOMParser();
      const doc = parser.parseFromString(currentAltoXml, 'application/xml');
      
      if (doc.querySelector('parsererror')) {
        throw new Error('Chybný XML formát');
      }
      
      // Create ALTO data structure
      const altoData = {
        pageWidth: 0,
        pageHeight: 0,
        printSpace: null,
        textBlocks: [],
        textLines: [],
        words: []
      };
      
      // Get page dimensions
      const pageEl = doc.getElementsByTagName('Page')[0];
      if (pageEl) {
        altoData.pageWidth = parseInt(pageEl.getAttribute('WIDTH') || '0');
        altoData.pageHeight = parseInt(pageEl.getAttribute('HEIGHT') || '0');
      }
      
      if (altoData.pageWidth === 0 || altoData.pageHeight === 0) {
        altoData.pageWidth = imageNaturalWidth;
        altoData.pageHeight = imageNaturalHeight;
      }
      
      // Parse elements
      const blocks = doc.getElementsByTagName('TextBlock');
      for (let i = 0; i < blocks.length; i++) {
        const block = blocks[i];
        altoData.textBlocks.push({
          x: parseInt(block.getAttribute('HPOS') || '0'),
          y: parseInt(block.getAttribute('VPOS') || '0'),
          w: parseInt(block.getAttribute('WIDTH') || '0'),
          h: parseInt(block.getAttribute('HEIGHT') || '0'),
          id: block.getAttribute('ID') || `block-${i}`,
          type: 'block'
        });
      }
      
      const lines = doc.getElementsByTagName('TextLine');
      for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        altoData.textLines.push({
          x: parseInt(line.getAttribute('HPOS') || '0'),
          y: parseInt(line.getAttribute('VPOS') || '0'),
          w: parseInt(line.getAttribute('WIDTH') || '0'),
          h: parseInt(line.getAttribute('HEIGHT') || '0'),
          id: line.getAttribute('ID') || `line-${i}`,
          type: 'line'
        });
      }
      
      const words = doc.getElementsByTagName('String');
      for (let i = 0; i < words.length; i++) {
        const word = words[i];
        altoData.words.push({
          x: parseInt(word.getAttribute('HPOS') || '0'),
          y: parseInt(word.getAttribute('VPOS') || '0'),
          w: parseInt(word.getAttribute('WIDTH') || '0'),
          h: parseInt(word.getAttribute('HEIGHT') || '0'),
          text: word.getAttribute('CONTENT') || '',
          id: word.getAttribute('ID') || `word-${i}`,
          type: 'word',
          originalText: word.getAttribute('CONTENT') || '' // Keep original for comparison
        });
      }
      
      currentAltoData = altoData;
      altoModified = false;
      updateSaveButtons();
      
      // Clear element map and selection
      elementIdMap.clear();
      selectedElementId = null;
      
      // Build element map for quick lookup
      altoData.words.forEach(word => elementIdMap.set(word.id, word));
      altoData.textLines.forEach(line => elementIdMap.set(line.id, line));
      altoData.textBlocks.forEach(block => elementIdMap.set(block.id, block));
      
      // Update info
      const elementCount = altoData.words.length + altoData.textLines.length + altoData.textBlocks.length;
      if (infoElementsEl) {
        infoElementsEl.textContent = `${elementCount} (${altoData.words.length} slov)`;
      }
      
      setStatus(`Stránka ${page.index} načtena`);
      
      // Update element list if in edit mode
      if (isEditingAlto && altoDisplayMode === 'elements') {
        populateAltoElementList();
      }
      
    } catch (err) {
      console.error('Chyba načítání ALTO:', err);
      setStatus(`Chyba: ${err.message}`, true);
      currentAltoData = null;
    }
  }

  // === DRAWING ===
  function drawAltoLayers() {
    if (!canvas || !ctx || !currentAltoData) return;
    
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    
    // Draw layers based on toggles
    if (layerPrintspace?.checked && currentAltoData.printSpace) {
      drawElement(currentAltoData.printSpace, 'rgba(0,255,0,0.1)', 'rgba(0,255,0,0.5)');
    }
    
    if (layerBlocks?.checked) {
      currentAltoData.textBlocks.forEach(block => {
        const isSelected = selectedElementId === block.id;
        drawElement(block, 
          isSelected ? 'rgba(79,195,247,0.2)' : 'rgba(0,128,255,0.05)',
          isSelected ? 'rgb(79,195,247)' : 'rgba(0,128,255,0.7)'
        );
      });
    }
    
    if (layerLines?.checked) {
      currentAltoData.textLines.forEach(line => {
        const isSelected = selectedElementId === line.id;
        drawElement(line, 
          isSelected ? 'rgba(79,195,247,0.2)' : 'rgba(255,165,0,0.05)',
          isSelected ? 'rgb(79,195,247)' : 'rgba(255,165,0,0.7)'
        );
      });
    }
    
    if (layerWords?.checked) {
      currentAltoData.words.forEach(word => {
        const isSelected = selectedElementId === word.id;
        const isHovered = currentHoveredElement?.id === word.id && !isEditingAlto;
        const isDragged = dragElement?.id === word.id && isEditingAlto;
        
        drawElement(word, 
          isSelected ? 'rgba(79,195,247,0.3)' : 
          isHovered ? 'rgba(255,255,0,0.15)' : 
          isDragged ? 'rgba(79,195,247,0.3)' : 'rgba(255,0,0,0.05)',
          isSelected || isDragged ? 'rgb(79,195,247)' : 'rgba(255,0,0,0.7)'
        );
        
        // Draw text for words at higher zoom
        if (word.text && scale > 1.5) {
          const coords = altoToCanvasCoordinates(word.x, word.y, word.w, word.h);
          ctx.save();
          ctx.fillStyle = 'rgba(255,255,255,0.9)';
          ctx.font = `${Math.max(8, 10 * scale)}px monospace`;
          ctx.textBaseline = 'top';
          ctx.fillText(word.text, coords.x + 2, coords.y + 2);
          ctx.restore();
        }
      });
    }
    
    // Draw resize handles for selected element in edit mode
    if (isEditingAlto && currentTool === 'select' && dragElement) {
      drawResizeHandles(dragElement);
    }
    
    // Draw creation rectangle
    if (isCreatingElement) {
      drawCreationRectangle();
    }
  }

  function drawElement(element, fillStyle, strokeStyle) {
    const coords = altoToCanvasCoordinates(element.x, element.y, element.w, element.h);
    
    ctx.save();
    ctx.fillStyle = fillStyle;
    ctx.strokeStyle = strokeStyle;
    ctx.lineWidth = 1;
    
    ctx.fillRect(coords.x, coords.y, coords.w, coords.h);
    ctx.strokeRect(coords.x, coords.y, coords.w, coords.h);
    ctx.restore();
  }

  function drawResizeHandles(element) {
    const coords = altoToCanvasCoordinates(element.x, element.y, element.w, element.h);
    const handleSize = 8;
    const halfHandle = handleSize / 2;
    
    ctx.save();
    ctx.fillStyle = 'rgb(79, 195, 247)';
    ctx.strokeStyle = 'rgb(255, 255, 255)';
    ctx.lineWidth = 1;
    
    const handles = [
      { x: coords.x, y: coords.y, type: 'nw' },
      { x: coords.x + coords.w / 2, y: coords.y, type: 'n' },
      { x: coords.x + coords.w, y: coords.y, type: 'ne' },
      { x: coords.x, y: coords.y + coords.h / 2, type: 'w' },
      { x: coords.x + coords.w, y: coords.y + coords.h / 2, type: 'e' },
      { x: coords.x, y: coords.y + coords.h, type: 'sw' },
      { x: coords.x + coords.w / 2, y: coords.y + coords.h, type: 's' },
      { x: coords.x + coords.w, y: coords.y + coords.h, type: 'se' }
    ];
    
    handles.forEach(handle => {
      ctx.fillRect(handle.x - halfHandle, handle.y - halfHandle, handleSize, handleSize);
      ctx.strokeRect(handle.x - halfHandle, handle.y - halfHandle, handleSize, handleSize);
    });
    
    ctx.restore();
  }

  function drawCreationRectangle() {
    const start = canvasToAltoCoordinates(createStartX, createStartY);
    const current = canvasToAltoCoordinates(createCurrentX, createCurrentY);
    
    const x = Math.min(start.x, current.x);
    const y = Math.min(start.y, current.y);
    const w = Math.abs(current.x - start.x);
    const h = Math.abs(current.y - start.y);
    
    const coords = altoToCanvasCoordinates(x, y, w, h);
    
    ctx.save();
    ctx.fillStyle = 'rgba(79, 195, 247, 0.2)';
    ctx.strokeStyle = 'rgb(79, 195, 247)';
    ctx.lineWidth = 2;
    ctx.setLineDash([5, 3]);
    
    ctx.fillRect(coords.x, coords.y, coords.w, coords.h);
    ctx.strokeRect(coords.x, coords.y, coords.w, coords.h);
    ctx.restore();
  }

  // === TEXT HIGHLIGHTING (VIEW MODE) ===
  function highlightTextInOCR(text) {
    if (!text || !fullOcrText || !ocrTextEl) return;
    
    clearHighlight();
    
    // Escape regex special characters
    const escapedText = text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const regex = new RegExp(`(${escapedText})`, 'gi');
    
    // Create highlighted HTML
    const highlighted = fullOcrText.replace(regex, '<span class="ocr-highlight">$1</span>');
    
    // Create temporary div for highlighting
    const tempDiv = document.createElement('div');
    tempDiv.className = 'ocr-text-highlight';
    tempDiv.style.cssText = window.getComputedStyle(ocrTextEl).cssText;
    tempDiv.style.whiteSpace = 'pre-wrap';
    tempDiv.style.fontFamily = "'Consolas', 'Monaco', monospace";
    tempDiv.style.fontSize = '12px';
    tempDiv.style.lineHeight = '1.4';
    tempDiv.style.color = 'var(--text-main)';
    tempDiv.style.backgroundColor = '#0d0f18';
    tempDiv.style.border = '1px solid var(--border-soft)';
    tempDiv.style.borderRadius = '6px';
    tempDiv.style.padding = '6px';
    tempDiv.style.overflowY = 'auto';
    tempDiv.style.minHeight = '150px';
    tempDiv.innerHTML = highlighted;
    tempDiv.id = 'ocrHighlightTemp';
    
    // Replace textarea with div
    const parent = ocrTextEl.parentNode;
    ocrTextEl.style.display = 'none';
    parent.insertBefore(tempDiv, ocrTextEl);
    
    // Scroll to first occurrence
    const firstMatch = fullOcrText.toLowerCase().indexOf(text.toLowerCase());
    if (firstMatch !== -1) {
      const tempText = fullOcrText.substring(0, firstMatch);
      const linesBefore = tempText.split('\n').length;
      tempDiv.scrollTop = linesBefore * 16;
    }
    
    // Set timer to clear highlight
    highlightTimer = setTimeout(clearHighlight, 3000);
  }

  function clearHighlight() {
    if (highlightTimer) {
      clearTimeout(highlightTimer);
      highlightTimer = null;
    }
    
    const tempDiv = document.getElementById('ocrHighlightTemp');
    if (tempDiv) {
      tempDiv.remove();
      ocrTextEl.style.display = 'block';
    }
    
    currentSelectedElement = null;
    drawAltoLayers();
  }

  // === ELEMENT INTERACTION ===
  function findElementAt(canvasX, canvasY) {
    if (!currentAltoData) return null;
    
    const altoCoords = canvasToAltoCoordinates(canvasX, canvasY);
    
    // Check words first (smallest)
    for (const word of currentAltoData.words) {
      if (altoCoords.x >= word.x && altoCoords.x <= word.x + word.w &&
          altoCoords.y >= word.y && altoCoords.y <= word.y + word.h) {
        return word;
      }
    }
    
    // Check lines
    for (const line of currentAltoData.textLines) {
      if (altoCoords.x >= line.x && altoCoords.x <= line.x + line.w &&
          altoCoords.y >= line.y && altoCoords.y <= line.y + line.h) {
        return line;
      }
    }
    
    // Check blocks
    for (const block of currentAltoData.textBlocks) {
      if (altoCoords.x >= block.x && altoCoords.x <= block.x + block.w &&
          altoCoords.y >= block.y && altoCoords.y <= block.y + block.h) {
        return block;
      }
    }
    
    return null;
  }

  function getResizeHandleAt(canvasX, canvasY, element) {
    if (!element) return null;
    
    const coords = altoToCanvasCoordinates(element.x, element.y, element.w, element.h);
    const handleSize = 12;
    const halfHandle = handleSize / 2;
    
    const handles = [
      { x: coords.x, y: coords.y, type: 'nw' },
      { x: coords.x + coords.w / 2, y: coords.y, type: 'n' },
      { x: coords.x + coords.w, y: coords.y, type: 'ne' },
      { x: coords.x, y: coords.y + coords.h / 2, type: 'w' },
      { x: coords.x + coords.w, y: coords.y + coords.h / 2, type: 'e' },
      { x: coords.x, y: coords.y + coords.h, type: 'sw' },
      { x: coords.x + coords.w / 2, y: coords.y + coords.h, type: 's' },
      { x: coords.x + coords.w, y: coords.y + coords.h, type: 'se' }
    ];
    
    for (const handle of handles) {
      if (Math.abs(canvasX - handle.x) <= halfHandle && 
          Math.abs(canvasY - handle.y) <= halfHandle) {
        return handle.type;
      }
    }
    
    return null;
  }

  // === ELEMENT SELECTION MANAGEMENT ===
  function selectElement(element) {
    if (!element) return;
    
    // Set as selected element
    selectedElementId = element.id;
    dragElement = element;
    
    // Update display
    drawAltoLayers();
    
    // Update element list selection
    updateElementListSelection();
    
    // Scroll to element in list if in elements mode
    if (isEditingAlto && altoDisplayMode === 'elements') {
      scrollToElementInList(element);
    }
  }

  function deselectElement() {
    selectedElementId = null;
    dragElement = null;
    
    // Update display
    drawAltoLayers();
    
    // Update element list selection
    updateElementListSelection();
  }

  function updateElementListSelection() {
    if (!altoElementList || altoDisplayMode !== 'elements') return;
    
    const items = altoElementList.querySelectorAll('.alto-element-item');
    items.forEach(item => {
      const isSelected = item.dataset.id === selectedElementId;
      item.classList.toggle('selected', isSelected);
      item.classList.toggle('active', false); // Remove temporary active class
    });
  }

  function scrollToElementInList(element) {
    if (!element || !altoElementList || altoDisplayMode !== 'elements') return;
    
    const item = altoElementList.querySelector(`.alto-element-item[data-id="${element.id}"]`);
    if (item) {
      // Scroll to the item
      item.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
      return true;
    }
    return false;
  }

  // === EDIT MODE ===
  function setTool(tool) {
    currentTool = tool;
    
    // Update tool buttons
    const buttons = altoTools.querySelectorAll('.alto-tool-btn');
    buttons.forEach(btn => {
      btn.classList.toggle('active', btn.dataset.tool === tool);
    });
    
    // Update cursor
    updateCanvasCursor();
  }

  function updateCanvasCursor() {
    if (!canvas) return;
    
    canvas.className = '';
    
    if (!isEditingAlto) return;
    
    switch(currentTool) {
      case 'select':
        if (dragElement) {
          canvas.classList.add('cursor-move');
        } else {
          canvas.classList.add('cursor-move');
        }
        break;
      case 'createWord':
      case 'createLine':
      case 'createBlock':
        canvas.classList.add('cursor-crosshair');
        break;
      case 'delete':
        canvas.style.cursor = 'default';
        break;
    }
  }

  function enterAltoEditMode() {
    isEditingAlto = true;
    editModeIndicator.style.display = 'block';
    altoEditor.style.display = 'flex';
    ocrTextEl.style.display = 'none';
    editAltoBtn.disabled = true;
    saveTxtBtn.disabled = true;
    
    showAltoElements();
    setTool('select');
    
    // Clear any previous selection
    deselectElement();
    
    showNotification('Editace ALTO aktivní', 3000);
  }

  function exitAltoEditMode() {
    isEditingAlto = false;
    editModeIndicator.style.display = 'none';
    altoEditor.style.display = 'none';
    ocrTextEl.style.display = 'block';
    editAltoBtn.disabled = false;
    saveTxtBtn.disabled = !txtModified;
    
    // Reset edit state but keep selection
    isDraggingElement = false;
    resizeHandle = null;
    isCreatingElement = false;
    
    clearHighlight();
    drawAltoLayers();
  }

  function exitEditModes() {
    if (isEditingAlto) exitAltoEditMode();
  }

  function updateSaveButtons() {
    if (saveAltoBtn) {
      saveAltoBtn.disabled = !altoModified;
    }
    if (saveTxtBtn) {
      saveTxtBtn.disabled = !txtModified;
    }
  }

  // === ELEMENT EDITING ===
  function populateAltoElementList() {
    if (!currentAltoData || !altoElementList) return;
    
    altoElementList.innerHTML = '';
    
    // Words
    currentAltoData.words.forEach((word, idx) => {
      altoElementList.appendChild(createElementItem(word, idx, 'word'));
    });
    
    // Lines
    currentAltoData.textLines.forEach((line, idx) => {
      altoElementList.appendChild(createElementItem(line, idx, 'line'));
    });
    
    // Blocks
    currentAltoData.textBlocks.forEach((block, idx) => {
      altoElementList.appendChild(createElementItem(block, idx, 'block'));
    });
    
    // Update selection after populating
    updateElementListSelection();
  }

  function createElementItem(element, index, type) {
    const item = document.createElement('div');
    item.className = 'alto-element-item';
    item.dataset.id = element.id;
    
    const typeName = type === 'word' ? 'Word' : type === 'line' ? 'Line' : 'Block';
    const displayText = type === 'word' ? element.text || '(prázdný)' : `${typeName} #${index + 1}`;
    
    item.innerHTML = `
      <div class="element-content">
        <strong>${displayText}</strong>
        <div class="element-type ${type}">${typeName} #${index + 1}</div>
      </div>
      <div class="element-actions">
        ${type === 'word' ? '<button class="element-action-btn edit" title="Editovat text">✏️</button>' : ''}
        <button class="element-action-btn delete" title="Smazat">🗑</button>
      </div>
      <div class="element-coords">
        ${Math.round(element.x)},${Math.round(element.y)}<br>
        ${Math.round(element.w)}×${Math.round(element.h)}
      </div>
    `;
    
    // Delete button
    const deleteBtn = item.querySelector('.delete');
    deleteBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      showDeleteConfirmation(element);
    });
    
    // Edit button (for words only)
    if (type === 'word') {
      const editBtn = item.querySelector('.edit');
      editBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        editWordText(element);
      });
    }
    
    // Select on click
    item.addEventListener('click', () => {
      selectElement(element);
    });
    
    return item;
  }

  function editWordText(word) {
    if (!word || word.type !== 'word') return;
    
    editingWordElement = word;
    textEditInput.value = word.text || '';
    
    textEditConfirm.onclick = () => {
      const newText = textEditInput.value.trim();
      if (newText !== word.text) {
        // Update word text
        word.text = newText;
        
        // Mark as modified
        altoModified = true;
        updateSaveButtons();
        
        // Synchronize with TXT
        synchronizeTxtFromAlto();
        
        // Update display
        drawAltoLayers();
        populateAltoElementList();
        
        showNotification('Text slova upraven', 2000);
      }
      hideModal(textEditModal);
    };
    
    textEditCancel.onclick = () => {
      hideModal(textEditModal);
    };
    
    showModal(textEditModal);
    textEditInput.focus();
    textEditInput.select();
  }

  function showDeleteConfirmation(element) {
    const typeName = element.type === 'word' ? 'slovo' : element.type === 'line' ? 'řádek' : 'blok';
    const content = element.type === 'word' ? `"${element.text}"` : element.id;
    deleteModalText.textContent = `Opravdu chcete smazat ${typeName} ${content}?`;
    
    let deleteConfirmed = false;
    
    deleteModalConfirm.onclick = () => {
      deleteConfirmed = true;
      deleteElement(element);
      hideModal(deleteModal);
    };
    
    deleteModalCancel.onclick = () => {
      hideModal(deleteModal);
    };
    
    showModal(deleteModal);
  }

  function deleteElement(element) {
    if (!currentAltoData) return;
    
    // Remove from array
    let array;
    if (element.type === 'word') array = currentAltoData.words;
    else if (element.type === 'line') array = currentAltoData.textLines;
    else array = currentAltoData.textBlocks;
    
    const index = array.findIndex(e => e.id === element.id);
    if (index !== -1) {
      array.splice(index, 1);
    }
    
    // Remove from element map
    elementIdMap.delete(element.id);
    
    // If deleting the selected element, deselect it
    if (selectedElementId === element.id) {
      deselectElement();
    }
    
    // Mark as modified
    altoModified = true;
    updateSaveButtons();
    
    // If deleting a word, synchronize TXT
    if (element.type === 'word') {
      synchronizeTxtFromAlto();
    }
    
    // Update display
    drawAltoLayers();
    populateAltoElementList();
    
    showNotification('Element smazán', 2000);
  }

  // === TEXT SYNCHRONIZATION ===
  function synchronizeTxtFromAlto() {
    if (!currentAltoData || !currentAltoData.words.length) return;
    
    // Sort words by position (top to bottom, left to right)
    const sortedWords = [...currentAltoData.words].sort((a, b) => {
      if (Math.abs(a.y - b.y) < a.h * 0.5) {
        // Same line (within half height)
        return a.x - b.x;
      }
      return a.y - b.y;
    });
    
    // Build text from words
    let newText = '';
    let lastY = -1;
    let lineStart = true;
    
    sortedWords.forEach((word, idx) => {
      if (word.text === undefined || word.text === null) return;
      
      // Check if new line
      if (lastY >= 0 && Math.abs(word.y - lastY) > word.h * 0.5) {
        newText += '\n';
        lineStart = true;
      }
      
      // Add word
      if (!lineStart && idx > 0) {
        newText += ' ';
      }
      newText += word.text;
      
      lastY = word.y;
      lineStart = false;
    });
    
    // Update TXT if different
    if (newText !== fullOcrText) {
      fullOcrText = newText;
      ocrTextEl.value = newText;
      // Don't set txtModified when syncing from ALTO
      // txtModified = true;
    }
  }

  function synchronizeAltoFromTxt(newText) {
    if (!currentAltoData || !currentAltoData.words.length) return;
    
    // Split text into words and lines
    const lines = newText.split('\n');
    const wordsFromText = [];
    let wordIndex = 0;
    
    lines.forEach(line => {
      const wordsInLine = line.trim().split(/\s+/);
      wordsInLine.forEach(word => {
        if (word.trim()) {
          wordsFromText.push(word.trim());
        }
      });
    });
    
    // Update existing words (keep positions, just update text)
    const sortedWords = [...currentAltoData.words].sort((a, b) => {
      if (Math.abs(a.y - b.y) < a.h * 0.5) {
        return a.x - b.x;
      }
      return a.y - b.y;
    });
    
    // Only update words that exist in both
    const minLength = Math.min(sortedWords.length, wordsFromText.length);
    
    for (let i = 0; i < minLength; i++) {
      if (sortedWords[i].text !== wordsFromText[i]) {
        sortedWords[i].text = wordsFromText[i];
        altoModified = true;
      }
    }
    
    // Mark as modified
    updateSaveButtons();
    
    // Update display
    drawAltoLayers();
    populateAltoElementList();
  }

  // === ELEMENT CREATION ===
  function startCreatingElement(canvasX, canvasY) {
    const altoCoords = canvasToAltoCoordinates(canvasX, canvasY);
    createStartX = canvasX;
    createStartY = canvasY;
    createCurrentX = canvasX;
    createCurrentY = canvasY;
    isCreatingElement = true;
    
    drawAltoLayers();
  }

  function updateCreatingElement(canvasX, canvasY) {
    if (!isCreatingElement) return;
    
    createCurrentX = canvasX;
    createCurrentY = canvasY;
    drawAltoLayers();
  }

  function finishCreatingElement() {
    if (!isCreatingElement) return;
    
    const start = canvasToAltoCoordinates(createStartX, createStartY);
    const current = canvasToAltoCoordinates(createCurrentX, createCurrentY);
    
    const x = Math.min(start.x, current.x);
    const y = Math.min(start.y, current.y);
    const w = Math.abs(current.x - start.x);
    const h = Math.abs(current.y - start.y);
    
    // Minimum size
    if (w < 10 || h < 10) {
      isCreatingElement = false;
      drawAltoLayers();
      return;
    }
    
    // Create new element
    const newElement = {
      x: Math.round(x),
      y: Math.round(y),
      w: Math.round(w),
      h: Math.round(h),
      id: `${currentTool.replace('create', '').toLowerCase()}-${Date.now()}`,
      type: currentTool.replace('create', '').toLowerCase()
    };
    
    if (newElement.type === 'word') {
      newElement.text = 'nové slovo';
      newElement.originalText = 'nové slovo';
      currentAltoData.words.push(newElement);
      elementIdMap.set(newElement.id, newElement);
      
      // Synchronize TXT
      synchronizeTxtFromAlto();
      txtModified = true;
      updateSaveButtons();
    } else if (newElement.type === 'line') {
      currentAltoData.textLines.push(newElement);
      elementIdMap.set(newElement.id, newElement);
    } else if (newElement.type === 'block') {
      currentAltoData.textBlocks.push(newElement);
      elementIdMap.set(newElement.id, newElement);
    }
    
    // Mark as modified
    altoModified = true;
    updateSaveButtons();
    
    // Reset creation state
    isCreatingElement = false;
    createStartX = 0;
    createStartY = 0;
    createCurrentX = 0;
    createCurrentY = 0;
    
    // Update display
    drawAltoLayers();
    populateAltoElementList();
    
    // Select the new element
    selectElement(newElement);
    
    showNotification('Nový element vytvořen', 2000);
    
    // Switch back to select tool
    setTool('select');
  }

  function cancelCreatingElement() {
    isCreatingElement = false;
    createStartX = 0;
    createStartY = 0;
    createCurrentX = 0;
    createCurrentY = 0;
    drawAltoLayers();
    setTool('select');
  }

  // === RESIZE HANDLING ===
  function handleResize(mouseAltoX, mouseAltoY) {
    if (!dragElement || !resizeHandle) return;
    
    switch(resizeHandle) {
      case 'nw':
        dragElement.w = dragStartWidth + (dragStartAltoX - mouseAltoX);
        dragElement.h = dragStartHeight + (dragStartAltoY - mouseAltoY);
        dragElement.x = mouseAltoX;
        dragElement.y = mouseAltoY;
        break;
      case 'n':
        dragElement.h = dragStartHeight + (dragStartAltoY - mouseAltoY);
        dragElement.y = mouseAltoY;
        break;
      case 'ne':
        dragElement.w = mouseAltoX - dragElement.x;
        dragElement.h = dragStartHeight + (dragStartAltoY - mouseAltoY);
        dragElement.y = mouseAltoY;
        break;
      case 'w':
        dragElement.w = dragStartWidth + (dragStartAltoX - mouseAltoX);
        dragElement.x = mouseAltoX;
        break;
      case 'e':
        dragElement.w = mouseAltoX - dragElement.x;
        break;
      case 'sw':
        dragElement.w = dragStartWidth + (dragStartAltoX - mouseAltoX);
        dragElement.h = mouseAltoY - dragElement.y;
        dragElement.x = mouseAltoX;
        break;
      case 's':
        dragElement.h = mouseAltoY - dragElement.y;
        break;
      case 'se':
        dragElement.w = mouseAltoX - dragElement.x;
        dragElement.h = mouseAltoY - dragElement.y;
        break;
    }
    
    // Minimum size
    dragElement.w = Math.max(10, dragElement.w);
    dragElement.h = Math.max(10, dragElement.h);
    
    // Keep within page bounds
    const altoWidth = currentAltoData.pageWidth || imageNaturalWidth;
    const altoHeight = currentAltoData.pageHeight || imageNaturalHeight;
    
    dragElement.x = Math.max(0, Math.min(altoWidth - dragElement.w, dragElement.x));
    dragElement.y = Math.max(0, Math.min(altoHeight - dragElement.h, dragElement.y));
  }

  // === EVENT HANDLERS ===
  function setupEventListeners() {
    // Page navigation
    createPageList();
    
    // Edit buttons
    if (editAltoBtn) {
      editAltoBtn.addEventListener('click', enterAltoEditMode);
    }
    
    if (cancelEditBtn) {
      cancelEditBtn.addEventListener('click', () => {
        if (altoModified) {
          showSaveConfirmation(exitAltoEditMode);
        } else {
          exitAltoEditMode();
        }
      });
    }
    
    // Save buttons
    if (saveTxtBtn) {
      saveTxtBtn.addEventListener('click', saveTxtFile);
    }
    
    if (saveAltoBtn) {
      saveAltoBtn.addEventListener('click', saveAltoFile);
    }
    
    // Tool buttons
    if (toolSelect) toolSelect.addEventListener('click', () => setTool('select'));
    if (toolCreateWord) toolCreateWord.addEventListener('click', () => setTool('createWord'));
    if (toolCreateLine) toolCreateLine.addEventListener('click', () => setTool('createLine'));
    if (toolCreateBlock) toolCreateBlock.addEventListener('click', () => setTool('createBlock'));
    if (toolDelete) toolDelete.addEventListener('click', () => setTool('delete'));
    
    // Display mode
    if (showXmlBtn) {
      showXmlBtn.addEventListener('click', () => {
        altoDisplayMode = 'xml';
        altoXmlEditor.style.display = 'flex';
        altoElementList.style.display = 'none';
        altoXmlText.value = currentAltoXml;
      });
    }
    
    if (showElementsBtn) {
      showElementsBtn.addEventListener('click', () => {
        altoDisplayMode = 'elements';
        altoXmlEditor.style.display = 'none';
        altoElementList.style.display = 'block';
        populateAltoElementList();
      });
    }
    
    // Textarea changes
    if (ocrTextEl) {
      ocrTextEl.addEventListener('input', () => {
        const newText = ocrTextEl.value;
        if (newText !== fullOcrText) {
          txtModified = true;
          updateSaveButtons();
          // Synchronize ALTO words from TXT changes
          synchronizeAltoFromTxt(newText);
        }
      });
    }
    
    if (altoXmlText) {
      altoXmlText.addEventListener('input', () => {
        altoModified = altoXmlText.value !== currentAltoXml;
        updateSaveButtons();
      });
    }
    
    // Zoom controls
    if (zoomInBtn) {
      zoomInBtn.addEventListener('click', () => {
        const rect = zoomWrapper.getBoundingClientRect();
        zoomToPoint(1.2, rect.left + rect.width / 2, rect.top + rect.height / 2);
      });
    }
    
    if (zoomOutBtn) {
      zoomOutBtn.addEventListener('click', () => {
        const rect = zoomWrapper.getBoundingClientRect();
        zoomToPoint(0.833, rect.left + rect.width / 2, rect.top + rect.height / 2);
      });
    }
    
    if (zoomResetBtn) {
      zoomResetBtn.addEventListener('click', () => {
        scale = 1.0;
        offsetX = 0;
        offsetY = 0;
        applyTransform();
      });
    }
    
    // Layer toggles
    [layerPrintspace, layerBlocks, layerLines, layerWords].forEach(toggle => {
      if (toggle) toggle.addEventListener('change', drawAltoLayers);
    });
    
    // Canvas events
    if (canvas) {
      // Zoom with mouse wheel
      zoomWrapper.addEventListener('wheel', (evt) => {
        evt.preventDefault();
        const factor = evt.deltaY > 0 ? 0.9 : 1.1;
        zoomToPoint(factor, evt.clientX, evt.clientY);
      });
      
      // Panning (view mode only)
      zoomWrapper.addEventListener('mousedown', (evt) => {
        if (evt.button === 0 && !isEditingAlto) {
          isDragging = true;
          lastMouseX = evt.clientX;
          lastMouseY = evt.clientY;
          canvas.style.cursor = 'grabbing';
        }
      });
      
      // Canvas mouse events
      canvas.addEventListener('mousedown', handleCanvasMouseDown);
      canvas.addEventListener('mousemove', handleCanvasMouseMove);
      canvas.addEventListener('mouseup', handleCanvasMouseUp);
      canvas.addEventListener('mouseleave', handleCanvasMouseLeave);
    }
    
    // Document events for panning
    document.addEventListener('mousemove', (evt) => {
      if (!isDragging || isEditingAlto) return;
      
      const dx = evt.clientX - lastMouseX;
      const dy = evt.clientY - lastMouseY;
      
      offsetX += dx;
      offsetY += dy;
      
      lastMouseX = evt.clientX;
      lastMouseY = evt.clientY;
      
      applyTransform();
    });
    
    document.addEventListener('mouseup', () => {
      if (isDragging) {
        isDragging = false;
        canvas.style.cursor = 'default';
      }
    });
    
    // Keyboard shortcuts
    document.addEventListener('keydown', handleKeyDown);
    
    // Window resize
    window.addEventListener('resize', () => {
      calculateImageFit();
      if (canvas) {
        canvas.width = containerWidth;
        canvas.height = containerHeight;
        drawAltoLayers();
      }
    });
  }

  function handleCanvasMouseDown(evt) {
    if (!currentAltoData || evt.button !== 0) return;
    
    const rect = canvas.getBoundingClientRect();
    const canvasX = evt.clientX - rect.left;
    const canvasY = evt.clientY - rect.top;
    
    if (isEditingAlto) {
      // EDIT MODE
      if (currentTool === 'select') {
        // Check for resize handle first
        if (dragElement) {
          const handle = getResizeHandleAt(canvasX, canvasY, dragElement);
          if (handle) {
            resizeHandle = handle;
            dragStartX = canvasX;
            dragStartY = canvasY;
            dragStartAltoX = dragElement.x;
            dragStartAltoY = dragElement.y;
            dragStartWidth = dragElement.w;
            dragStartHeight = dragElement.h;
            isDraggingElement = true;
            return;
          }
        }
        
        // Check for element click
        const element = findElementAt(canvasX, canvasY);
        if (element) {
          selectElement(element);
          const altoCoords = canvasToAltoCoordinates(canvasX, canvasY);
          dragStartX = canvasX;
          dragStartY = canvasY;
          dragStartAltoX = element.x;
          dragStartAltoY = element.y;
          isDraggingElement = true;
          resizeHandle = null;
        } else {
          // Clicked on empty space - deselect
          deselectElement();
        }
      } else if (currentTool.startsWith('create')) {
        // Start creating new element
        startCreatingElement(canvasX, canvasY);
      } else if (currentTool === 'delete') {
        // Delete element on click
        const element = findElementAt(canvasX, canvasY);
        if (element) {
          showDeleteConfirmation(element);
        }
      }
    } else {
      // VIEW MODE - highlight text
      const element = findElementAt(canvasX, canvasY);
      if (element && element.type === 'word') {
        currentSelectedElement = element;
        highlightTextInOCR(element.text);
        drawAltoLayers();
      }
    }
  }

  function handleCanvasMouseMove(evt) {
    if (!currentAltoData) return;
    
    const rect = canvas.getBoundingClientRect();
    const canvasX = evt.clientX - rect.left;
    const canvasY = evt.clientY - rect.top;
    
    // Update hover effect in view mode
    if (!isEditingAlto) {
      const element = findElementAt(canvasX, canvasY);
      if (element !== currentHoveredElement) {
        currentHoveredElement = element;
        drawAltoLayers();
      }
      return;
    }
    
    // EDIT MODE
    if (isDraggingElement && dragElement) {
      // Drag element or resize
      const altoCoords = canvasToAltoCoordinates(canvasX, canvasY);
      
      if (resizeHandle) {
        // Resize
        handleResize(altoCoords.x, altoCoords.y);
      } else {
        // Move
        const deltaCanvasX = canvasX - dragStartX;
        const deltaCanvasY = canvasY - dragStartY;
        
        const deltaAltoX = deltaCanvasX / (imageScale * scale) * (currentAltoData.pageWidth / imageNaturalWidth);
        const deltaAltoY = deltaCanvasY / (imageScale * scale) * (currentAltoData.pageHeight / imageNaturalHeight);
        
        dragElement.x = dragStartAltoX + deltaAltoX;
        dragElement.y = dragStartAltoY + deltaAltoY;
        
        // Keep in bounds
        const altoWidth = currentAltoData.pageWidth || imageNaturalWidth;
        const altoHeight = currentAltoData.pageHeight || imageNaturalHeight;
        
        dragElement.x = Math.max(0, Math.min(altoWidth - dragElement.w, dragElement.x));
        dragElement.y = Math.max(0, Math.min(altoHeight - dragElement.h, dragElement.y));
      }
      
      drawAltoLayers();
    } else if (isCreatingElement) {
      // Update creation rectangle
      updateCreatingElement(canvasX, canvasY);
    } else if (currentTool === 'select') {
      // Update cursor based on hover
      const element = findElementAt(canvasX, canvasY);
      if (element) {
        const handle = getResizeHandleAt(canvasX, canvasY, element);
        if (handle) {
          canvas.className = `cursor-resize-${handle}`;
        } else {
          canvas.className = 'cursor-move';
        }
      } else {
        updateCanvasCursor();
      }
    }
  }

  function handleCanvasMouseUp() {
    if (isEditingAlto) {
      if (isDraggingElement && dragElement) {
        // Mark as modified
        altoModified = true;
        updateSaveButtons();
        
        isDraggingElement = false;
        resizeHandle = null;
        
        // Update element list
        populateAltoElementList();
        
        showNotification('Element upraven', 1000);
      } else if (isCreatingElement) {
        finishCreatingElement();
      }
    }
  }

  function handleCanvasMouseLeave() {
    if (isEditingAlto) {
      if (isDraggingElement) {
        isDraggingElement = false;
        resizeHandle = null;
      }
      updateCanvasCursor();
    }
  }

  function handleKeyDown(evt) {
    // Escape to exit modes
    if (evt.key === 'Escape') {
      if (isEditingAlto) {
        if (isCreatingElement) {
          cancelCreatingElement();
        } else {
          // Deselect current element
          deselectElement();
        }
      } else {
        clearHighlight();
      }
      evt.preventDefault();
    }
    
    // Tool shortcuts in edit mode
    if (isEditingAlto) {
      switch(evt.key.toLowerCase()) {
        case 'v':
          setTool('select');
          break;
        case 'w':
          setTool('createWord');
          break;
        case 'l':
          setTool('createLine');
          break;
        case 'b':
          setTool('createBlock');
          break;
        case 'd':
          setTool('delete');
          break;
        case 'delete':
          if (dragElement) {
            showDeleteConfirmation(dragElement);
          }
          break;
        case 'e':
          if (dragElement && dragElement.type === 'word') {
            editWordText(dragElement);
          }
          break;
      }
    }
    
    // Page navigation
    const pages = MANIFEST.pages || [];
    if (evt.key === 'ArrowUp' || evt.key === 'ArrowDown') {
      if (txtModified || altoModified) {
        showSaveConfirmation(() => {
          const newIndex = evt.key === 'ArrowUp' ? currentPageIndex - 1 : currentPageIndex + 1;
          if (newIndex >= 0 && newIndex < pages.length) {
            selectPage(newIndex);
          }
        });
      } else {
        const newIndex = evt.key === 'ArrowUp' ? currentPageIndex - 1 : currentPageIndex + 1;
        if (newIndex >= 0 && newIndex < pages.length) {
          selectPage(newIndex);
        }
      }
      evt.preventDefault();
    }
  }

  // === SAVE FUNCTIONS ===
  function showSaveConfirmation(callback) {
    const page = MANIFEST.pages[currentPageIndex];
    if (!page) return;
    
    let message = 'Máte neuložené změny. Chcete je uložit?';
    if (txtModified && altoModified) {
      message = `Máte neuložené změny v souborech:\n• ${page.txtName}\n• ${page.altoName}\n\nChcete změny uložit?`;
    } else if (txtModified) {
      message = `Máte neuložené změny v souboru:\n• ${page.txtName}\n\nChcete změny uložit?`;
    } else if (altoModified) {
      message = `Máte neuložené změny v souboru:\n• ${page.altoName}\n\nChcete změny uložit?`;
    }
    
    saveModalText.textContent = message;
    
    saveModalConfirm.onclick = () => {
      saveAllChanges().then(() => {
        hideModal(saveModal);
        if (callback) callback();
      });
    };
    
    saveModalCancel.onclick = () => {
      hideModal(saveModal);
      if (callback) callback();
    };
    
    showModal(saveModal);
  }

  async function saveTxtFile() {
    const page = MANIFEST.pages[currentPageIndex];
    if (!page || !page.txtName) return false;
    
    try {
      const newText = ocrTextEl.value;
      
      // Save to file (client-side download)
      const blob = new Blob([newText], { type: 'text/plain;charset=utf-8' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = page.txtName;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
      
      // Update state
      fullOcrText = newText;
      originalFullOcrText = newText;
      txtModified = false;
      updateSaveButtons();
      
      showNotification('TXT soubor připraven ke stažení', 2000);
      return true;
    } catch (err) {
      console.error('Chyba ukládání TXT:', err);
      showNotification('Chyba při ukládání TXT', 3000);
      return false;
    }
  }

  async function saveAltoFile() {
    const page = MANIFEST.pages[currentPageIndex];
    if (!page || !page.altoName) return false;
    
    try {
      // TODO: Generate XML from currentAltoData
      const blob = new Blob([currentAltoXml], { type: 'application/xml;charset=utf-8' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = page.altoName;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
      
      altoModified = false;
      updateSaveButtons();
      
      showNotification('ALTO soubor připraven ke stažení', 2000);
      return true;
    } catch (err) {
      console.error('Chyba ukládání ALTO:', err);
      showNotification('Chyba při ukládání ALTO', 3000);
      return false;
    }
  }

  async function saveAllChanges() {
    let success = true;
    
    if (txtModified) {
      success = success && await saveTxtFile();
    }
    
    if (altoModified) {
      success = success && await saveAltoFile();
    }
    
    return success;
  }

  // === INITIALIZATION ===
  function init() {
    setupEventListeners();
    
    // Set metadata
    const metaBatch = document.getElementById('metaBatch');
    const metaLang = document.getElementById('metaLang');
    const metaAlto = document.getElementById('metaAlto');
    
    if (metaBatch) metaBatch.textContent = 'Dávka: ' + MANIFEST.batchName;
    if (metaLang) metaLang.textContent = 'Jazyk OCR: ' + MANIFEST.lang;
    if (metaAlto) metaAlto.textContent = 'ALTO: ' + MANIFEST.altoVersion;
    
    // Load first page
    if (MANIFEST.pages && MANIFEST.pages.length > 0) {
      selectPage(0);
    } else {
      setStatus('Žádné stránky k zobrazení', true);
    }
  }

  // Start the application
  init();
})();
</script>
</body>
</html>
"#,
    );

    let output_path = logs_dir.join("index.html");
    println!("DEBUG: Ukládám HTML do: {:?}", output_path);
    
    fs::write(&output_path, html)
        .with_context(|| format!("Nelze zapsat HTML soubor: {:?}", output_path))?;
    
    println!("DEBUG: HTML úspěšně vygenerován: {:?}", output_path);
    println!("DEBUG: Velikost HTML: cca {} znaků", manifest_js.len() + 50000); // Přibližná velikost
    
    Ok(())
}