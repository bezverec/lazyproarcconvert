use anyhow::{anyhow, Context, Result};
use clap::Parser;
use chrono::Local;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
        MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, ListState},
    Frame, Terminal,
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

mod blake3;
mod manifest;
mod previews;
mod html;

use manifest::build_manifest_for_batch;
use previews::generate_webp_previews;
use crate::html::write_html_report;

/// LazyProArcConvert
/// Batch wrapper kolem Grok JP2 komprese + Tesseract OCR/ALTO,
/// s Ratatui TUI rozhraním.
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Kořenový vstupní adresář (v něm budou dávky jako podadresáře)
    #[arg(short, long)]
    input: Option<PathBuf>,

    /// Kořenový výstupní adresář (pro všechny dávky, každá dávka má svůj podadresář)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Počáteční index (####) pro první stránku v první dávce
    #[arg(long, default_value_t = 1)]
    start_index: u32,

    /// Šířka indexu (počet číslic)
    #[arg(long, default_value_t = 4)]
    digits: usize,

    /// Jazyk pro Tesseract (např. ces, eng, eng+ces)
    #[arg(long, default_value = "ces")]
    lang: String,

    /// Verze ALTO (např. 4.4, 4.0, 3.0)
    #[arg(long, default_value = "4.4")]
    alto_version: String,

    /// Cesta / název binárky Grok (grk_compress).
    /// "auto" = ./grok/bin/grk_compress(.exe) nebo ./grok/grk_compress(.exe), jinak PATH.
    #[arg(long, default_value = "auto")]
    grok_bin: String,

    /// Cesta / název binárky Tesseract
    /// "auto" = nejprve zkusí lokální složku programu, pak PATH.
    #[arg(long, default_value = "auto")]
    tess_bin: String,

    /// Dry-run – pouze vypíše příkazy, nic nespustí
    #[arg(long)]
    dry_run: bool,

    /// Cesta k tessdata adresáři (pokud se nenalézá automaticky)
    #[arg(long)]
    tessdata_dir: Option<PathBuf>,

    /// Vynutit použití lokálního Tesseractu (ignorovat PATH)
    #[arg(long)]
    force_local_tess: bool,
}

#[derive(Debug, Clone)]
enum JobStatus {
    Pending,
    Processing,
    Done,
    Failed(String),
    AlreadyDone, // Nový stav - dávka byla již dříve zpracována
}

#[derive(Debug, Clone)]
struct BatchJob {
    /// Adresář dávky (input/něco)
    dir: PathBuf,
    /// První index stránky pro tuto dávky
    index_start: u32,
    /// Počet TIFF souborů v dávce
    file_count: usize,
    /// Stav dávky
    status: JobStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiMode {
    Normal,
    EditInput,
    EditOutput,
    LanguageMenu,
    AltoVersionMenu,
    CustomLangInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusedPane {
    Jobs,
    Detail,
    Log,
}

#[derive(Debug, Clone)]
enum ToolStatus {
    Ok(String),
    Error(String),
}

struct App {
    args: Args,
    input_root: PathBuf,
    output_root: PathBuf,
    jobs: Vec<BatchJob>,
    selected: usize,
    do_master: bool,
    do_user: bool,
    do_txt: bool,
    do_alto: bool,
    log_lines: Vec<String>,
    mode: UiMode,
    edit_buffer: String,
    grok_path: PathBuf,
    grok_status: ToolStatus,
    grok_version: String,
    tess_path: PathBuf,
    tess_source: String, // Uložíme odkud pochází
    tess_status: ToolStatus,
    focus: FocusedPane,
    detail_scroll: usize,
    log_scroll: usize,
    language_list_state: ListState,
    alto_version_list_state: ListState,
    available_languages: Vec<(String, String)>, // (kód, název)
    available_alto_versions: Vec<String>,
    custom_lang_input: String,
    tessdata_dir: Option<PathBuf>,
}

impl App {
    fn new(
        args: Args,
        input_root: PathBuf,
        output_root: PathBuf,
        jobs: Vec<BatchJob>,
        grok_path: PathBuf,
        grok_status: ToolStatus,
        grok_version: String,
        tess_path: PathBuf,
        tess_source: String,
        tess_status: ToolStatus,
        tessdata_dir: Option<PathBuf>,
    ) -> Self {
        // Seznam dostupných jazyků pro Tesseract s názvy
        let available_languages = vec![
            ("afr".to_string(), "afrikánština".to_string()),
            ("amh".to_string(), "amharština".to_string()),
            ("ara".to_string(), "arabština".to_string()),
            ("asm".to_string(), "ásámština".to_string()),
            ("aze".to_string(), "ázerbájdžánština".to_string()),
            ("aze_cyrl".to_string(), "ázerbájdžánština (cyrilice)".to_string()),
            ("bel".to_string(), "běloruština".to_string()),
            ("ben".to_string(), "bengálština".to_string()),
            ("bod".to_string(), "tibetština".to_string()),
            ("bos".to_string(), "bosenština".to_string()),
            ("bre".to_string(), "bretonština".to_string()),
            ("bul".to_string(), "bulharština".to_string()),
            ("cat".to_string(), "katalánština".to_string()),
            ("ceb".to_string(), "cebuánština".to_string()),
            ("ces".to_string(), "čeština".to_string()),
            ("chi_sim".to_string(), "čínština (zjednodušená)".to_string()),
            ("chi_sim_vert".to_string(), "čínština (zjednodušená, vertikální)".to_string()),
            ("chi_tra".to_string(), "čínština (tradiční)".to_string()),
            ("chi_tra_vert".to_string(), "čínština (tradiční, vertikální)".to_string()),
            ("chr".to_string(), "čerokézština".to_string()),
            ("cos".to_string(), "korsičtina".to_string()),
            ("cym".to_string(), "velština".to_string()),
            ("dan".to_string(), "dánština".to_string()),
            ("deu".to_string(), "němčina".to_string()),
            ("deu_latf".to_string(), "němčina (fraktur)".to_string()),
            ("div".to_string(), "maledivština".to_string()),
            ("dzo".to_string(), "dzongkä".to_string()),
            ("ell".to_string(), "řečtina".to_string()),
            ("eng".to_string(), "angličtina".to_string()),
            ("enm".to_string(), "střední angličtina".to_string()),
            ("epo".to_string(), "esperanto".to_string()),
            ("equ".to_string(), "matematika".to_string()),
            ("est".to_string(), "estonština".to_string()),
            ("eus".to_string(), "baskičtina".to_string()),
            ("fao".to_string(), "faerština".to_string()),
            ("fas".to_string(), "perština".to_string()),
            ("fil".to_string(), "filipínština".to_string()),
            ("fin".to_string(), "finština".to_string()),
            ("fra".to_string(), "francouzština".to_string()),
            ("frm".to_string(), "střední francouzština".to_string()),
            ("fry".to_string(), "fríština".to_string()),
            ("gla".to_string(), "skotská gaelština".to_string()),
            ("gle".to_string(), "irština".to_string()),
            ("glg".to_string(), "galicijština".to_string()),
            ("grc".to_string(), "stará řečtina".to_string()),
            ("guj".to_string(), "gudžarátština".to_string()),
            ("hat".to_string(), "haitština".to_string()),
            ("heb".to_string(), "hebrejština".to_string()),
            ("hin".to_string(), "hindština".to_string()),
            ("hrv".to_string(), "chorvatština".to_string()),
            ("hun".to_string(), "maďarština".to_string()),
            ("hye".to_string(), "arménština".to_string()),
            ("iku".to_string(), "inuktitutština".to_string()),
            ("ind".to_string(), "indonéština".to_string()),
            ("isl".to_string(), "islandština".to_string()),
            ("ita".to_string(), "italština".to_string()),
            ("ita_old".to_string(), "stará italština".to_string()),
            ("jav".to_string(), "jávština".to_string()),
            ("jpn".to_string(), "japonština".to_string()),
            ("jpn_vert".to_string(), "japonština (vertikální)".to_string()),
            ("kan".to_string(), "kannadština".to_string()),
            ("kat".to_string(), "gruzínština".to_string()),
            ("kat_old".to_string(), "stará gruzínština".to_string()),
            ("kaz".to_string(), "kazaština".to_string()),
            ("khm".to_string(), "khmerština".to_string()),
            ("kir".to_string(), "kyrgyzština".to_string()),
            ("kmr".to_string(), "kurdština (kurmanji)".to_string()),
            ("kor".to_string(), "korejština".to_string()),
            ("kor_vert".to_string(), "korejština (vertikální)".to_string()),
            ("lao".to_string(), "laoština".to_string()),
            ("lat".to_string(), "latina".to_string()),
            ("lav".to_string(), "lotyština".to_string()),
            ("lit".to_string(), "litevština".to_string()),
            ("ltz".to_string(), "lucemburština".to_string()),
            ("mal".to_string(), "malajálamština".to_string()),
            ("mar".to_string(), "maráthština".to_string()),
            ("mkd".to_string(), "makedonština".to_string()),
            ("mlt".to_string(), "maltština".to_string()),
            ("mon".to_string(), "mongolština".to_string()),
            ("mri".to_string(), "maorština".to_string()),
            ("msa".to_string(), "malajština".to_string()),
            ("mya".to_string(), "barmština".to_string()),
            ("nep".to_string(), "nepálština".to_string()),
            ("nld".to_string(), "nizozemština".to_string()),
            ("nor".to_string(), "norština".to_string()),
            ("oci".to_string(), "okcitánština".to_string()),
            ("ori".to_string(), "urijština".to_string()),
            ("osd".to_string(), "orientace a detekce skriptu".to_string()),
            ("pan".to_string(), "pandžábština".to_string()),
            ("pol".to_string(), "polština".to_string()),
            ("por".to_string(), "portugalština".to_string()),
            ("pus".to_string(), "paštština".to_string()),
            ("que".to_string(), "kečuánština".to_string()),
            ("ron".to_string(), "rumunština".to_string()),
            ("rus".to_string(), "ruština".to_string()),
            ("san".to_string(), "sanskrt".to_string()),
            ("sin".to_string(), "sinhálština".to_string()),
            ("slk".to_string(), "slovenština".to_string()),
            ("slv".to_string(), "slovinština".to_string()),
            ("snd".to_string(), "sindhština".to_string()),
            ("spa".to_string(), "španělština".to_string()),
            ("spa_old".to_string(), "stará španělština".to_string()),
            ("sqi".to_string(), "albánština".to_string()),
            ("srp".to_string(), "srbština (cyrilice)".to_string()),
            ("srp_latn".to_string(), "srbština (latinka)".to_string()),
            ("sun".to_string(), "sundánština".to_string()),
            ("swa".to_string(), "svahilština".to_string()),
            ("swe".to_string(), "švédština".to_string()),
            ("syr".to_string(), "syrština".to_string()),
            ("tam".to_string(), "tamilština".to_string()),
            ("tat".to_string(), "tatarština".to_string()),
            ("tel".to_string(), "telugština".to_string()),
            ("tgk".to_string(), "tádžičtina".to_string()),
            ("tha".to_string(), "thajština".to_string()),
            ("tir".to_string(), "tigrinijština".to_string()),
            ("ton".to_string(), "tongánština".to_string()),
            ("tur".to_string(), "turečtina".to_string()),
            ("uig".to_string(), "ujgurština".to_string()),
            ("ukr".to_string(), "ukrajinština".to_string()),
            ("urd".to_string(), "urdština".to_string()),
            ("uzb".to_string(), "uzbečtina (latinka)".to_string()),
            ("uzb_cyrl".to_string(), "uzbečtina (cyrilice)".to_string()),
            ("vie".to_string(), "vietnamština".to_string()),
            ("yid".to_string(), "jidiš".to_string()),
            ("yor".to_string(), "jorubština".to_string()),
            // Skripty (můžete je přidat jako speciální jazyky)
            ("script/Arabic".to_string(), "arabské písmo".to_string()),
            ("script/Armenian".to_string(), "arménské písmo".to_string()),
            ("script/Bengali".to_string(), "bengálské písmo".to_string()),
            ("script/Canadian_Aboriginal".to_string(), "domorodé kanadské písmo".to_string()),
            ("script/Cherokee".to_string(), "čerokézské písmo".to_string()),
            ("script/Cyrillic".to_string(), "cyrilice".to_string()),
            ("script/Devanagari".to_string(), "dévanágarí".to_string()),
            ("script/Ethiopic".to_string(), "etiopské písmo".to_string()),
            ("script/Fraktur".to_string(), "fraktura".to_string()),
            ("script/Georgian".to_string(), "gruzínské písmo".to_string()),
            ("script/Greek".to_string(), "řecké písmo".to_string()),
            ("script/Gujarati".to_string(), "gudžarátské písmo".to_string()),
            ("script/Gurmukhi".to_string(), "gurmukhí".to_string()),
            ("script/HanS".to_string(), "čínské písmo (zjednodušené)".to_string()),
            ("script/HanS_vert".to_string(), "čínské písmo (zjednodušené, vertikální)".to_string()),
            ("script/HanT".to_string(), "čínské písmo (tradiční)".to_string()),
            ("script/HanT_vert".to_string(), "čínské písmo (tradiční, vertikální)".to_string()),
            ("script/Hangul".to_string(), "hangul (korejské písmo)".to_string()),
            ("script/Hangul_vert".to_string(), "hangul (korejské písmo, vertikální)".to_string()),
            ("script/Hebrew".to_string(), "hebrejské písmo".to_string()),
            ("script/Japanese".to_string(), "japonské písmo".to_string()),
            ("script/Japanese_vert".to_string(), "japonské písmo (vertikální)".to_string()),
            ("script/Kannada".to_string(), "kannadské písmo".to_string()),
            ("script/Khmer".to_string(), "khmerské písmo".to_string()),
            ("script/Lao".to_string(), "laoské písmo".to_string()),
            ("script/Latin".to_string(), "latinka".to_string()),
            ("script/Malayalam".to_string(), "malajálamské písmo".to_string()),
            ("script/Myanmar".to_string(), "myanmarské písmo".to_string()),
            ("script/Oriya".to_string(), "orijské písmo".to_string()),
            ("script/Sinhala".to_string(), "sinhálské písmo".to_string()),
            ("script/Syriac".to_string(), "syrské písmo".to_string()),
            ("script/Tamil".to_string(), "tamilské písmo".to_string()),
            ("script/Telugu".to_string(), "telugské písmo".to_string()),
            ("script/Thaana".to_string(), "thaana (maledivské písmo)".to_string()),
            ("script/Thai".to_string(), "thajské písmo".to_string()),
            ("script/Tibetan".to_string(), "tibetské písmo".to_string()),
            ("script/Vietnamese".to_string(), "vietnamské písmo".to_string()),
        ];

        // Seznam dostupných ALTO verzí
        let available_alto_versions = vec![
            "4.4".to_string(),
            "4.3".to_string(),
            "4.2".to_string(),
            "4.1".to_string(),
            "4.0".to_string(),
            "3.0".to_string(),
            "2.1".to_string(),
            "2.0".to_string(),
            "1.4".to_string(),
            "1.3".to_string(),
            "1.2".to_string(),
            "1.1".to_string(),
            "1.0".to_string(),
        ];

        let mut language_list_state = ListState::default();
        let lang_index = available_languages
            .iter()
            .position(|(code, _)| code == &args.lang)
            .unwrap_or(0);
        language_list_state.select(Some(lang_index));

        let mut alto_version_list_state = ListState::default();
        let alto_index = available_alto_versions
            .iter()
            .position(|v| v == &args.alto_version)
            .unwrap_or(0);
        alto_version_list_state.select(Some(alto_index));

        let mut app = Self {
            args,
            input_root,
            output_root,
            jobs,
            selected: 0,
            do_master: true,
            do_user: true,
            do_txt: true,
            do_alto: true,
            log_lines: vec![],
            mode: UiMode::Normal,
            edit_buffer: String::new(),
            grok_path,
            grok_status,
            grok_version,
            tess_path,
            tess_source,
            tess_status,
            focus: FocusedPane::Jobs,
            detail_scroll: 0,
            log_scroll: 0,
            language_list_state,
            alto_version_list_state,
            available_languages,
            available_alto_versions,
            custom_lang_input: String::new(),
            tessdata_dir,
        };

        // Kontrola, které dávky jsou již zpracovány
        update_jobs_status_on_start(
            &mut app.jobs,
            &app.output_root,
            &app.input_root,
            app.do_master,
            app.do_user,
            app.do_txt,
            app.do_alto,
            app.args.digits,
        );

        // Úvodní log
        app.push_log("════════════════════════════════════════".to_string());
        app.push_log("LazyProArcConvert – JP2 + OCR/ALTO batch".to_string());
        app.push_log("════════════════════════════════════════".to_string());
        app.push_log("".to_string());
        app.push_log("OVLÁDÁNÍ:".to_string());
        app.push_log("  ↑/↓: výběr dávky".to_string());
        app.push_log("  Enter: zpracovat vybranou dávku".to_string());
        app.push_log("  a: zpracovat všechny Pending dávky".to_string());
        app.push_log("  m/u/t/l: přepnout AC/UC/TXT/ALTO".to_string());
        app.push_log("  L: výběr jazyka".to_string());
        app.push_log("  A: výběr ALTO verze".to_string());
        app.push_log("  Tab/Shift+Tab: přepnout fokus (Dávky/Detail/Log)".to_string());
        app.push_log("  PgUp/PgDn: stránkování ve fokussovaném panelu".to_string());
        app.push_log("  i/o: změnit input/output root".to_string());
        app.push_log("  F: vynutit přepracování vybrané dávky".to_string());
        app.push_log("  R: vynutit přepracování všech hotových dávek".to_string());
        app.push_log("  q: konec".to_string());
        app.push_log("".to_string());
        
        // Log nalezených nástrojů
        app.push_log("=== NALEZENÉ NÁSTROJE ===".to_string());
        app.push_log(format!("Grok: {}", app.grok_path.display()));
        
        app.push_log(format!("Tesseract: {}", app.tess_path.display()));
        app.push_log(format!("Tesseract zdroj: {}", app.tess_source));
        
        if let Some(ref td) = app.tessdata_dir {
            app.push_log(format!("Tessdata: {}", td.display()));
        } else {
            if let Some(found) = find_tessdata_parent_dir(&app.tess_path) {
                app.push_log(format!("Tessdata (auto): {}", found.display()));
            } else {
                app.push_log("Tessdata: Nenalezeno! OCR nemusí fungovat.".to_string());
            }
        }
        
        // Log hledání v PATH
        app.push_log("=== KONTROLA PATH ===".to_string());
        if let Some(path_tess) = find_tesseract_in_path() {
            app.push_log(format!("Tesseract v PATH: {}", path_tess.display()));
            if app.tess_path == path_tess {
                app.push_log("✓ Používáme Tesseract z PATH".to_string());
            } else {
                app.push_log("⚠ Používáme jiný Tesseract než v PATH".to_string());
                app.push_log(format!("  Důvod: {}", app.tess_source));
            }
        } else {
            app.push_log("Tesseract nenalezen v PATH".to_string());
        }
        
        app.push_log("=== NASTAVENÍ ===".to_string());
        app.push_log(format!("Jazyk: {}", app.args.lang));
        app.push_log(format!("ALTO verze: {}", app.args.alto_version));
        app.push_log(format!("Force local Tesseract: {}", app.args.force_local_tess));
        app.push_log("Ready.".to_string());

        app
    }

    fn next_job(&mut self) {
        if self.jobs.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.jobs.len();
    }

    fn prev_job(&mut self) {
        if self.jobs.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.jobs.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    fn up(&mut self) {
        match self.focus {
            FocusedPane::Jobs => self.prev_job(),
            FocusedPane::Detail => self.scroll_detail_up(1),
            FocusedPane::Log => self.scroll_log_up(1),
        }
    }

    fn down(&mut self) {
        match self.focus {
            FocusedPane::Jobs => self.next_job(),
            FocusedPane::Detail => self.scroll_detail_down(1),
            FocusedPane::Log => self.scroll_log_down(1),
        }
    }

    fn page_up(&mut self) {
        match self.focus {
            FocusedPane::Jobs => {
                for _ in 0..10 {
                    self.prev_job();
                }
            }
            FocusedPane::Detail => self.scroll_detail_up(10),
            FocusedPane::Log => self.scroll_log_up(10),
        }
    }

    fn page_down(&mut self) {
        match self.focus {
            FocusedPane::Jobs => {
                for _ in 0..10 {
                    self.next_job();
                }
            }
            FocusedPane::Detail => self.scroll_detail_down(10),
            FocusedPane::Log => self.scroll_log_down(10),
        }
    }

    fn scroll_current_up(&mut self, lines: usize) {
        match self.focus {
            FocusedPane::Jobs => {
                for _ in 0..lines {
                    self.prev_job();
                }
            }
            FocusedPane::Detail => self.scroll_detail_up(lines),
            FocusedPane::Log => self.scroll_log_up(lines),
        }
    }

    fn scroll_current_down(&mut self, lines: usize) {
        match self.focus {
            FocusedPane::Jobs => {
                for _ in 0..lines {
                    self.next_job();
                }
            }
            FocusedPane::Detail => self.scroll_detail_down(lines),
            FocusedPane::Log => self.scroll_log_down(lines),
        }
    }

    fn scroll_detail_up(&mut self, lines: usize) {
        self.detail_scroll = self.detail_scroll.saturating_sub(lines);
    }

    fn scroll_detail_down(&mut self, lines: usize) {
        self.detail_scroll = self.detail_scroll.saturating_add(lines);
    }

    fn scroll_log_up(&mut self, lines: usize) {
        self.log_scroll = self.log_scroll.saturating_sub(lines);
    }

    fn scroll_log_down(&mut self, lines: usize) {
        self.log_scroll = self.log_scroll.saturating_add(lines);
    }

    fn scroll_language_menu_up(&mut self) {
        if let Some(selected) = self.language_list_state.selected() {
            if selected > 0 {
                self.language_list_state.select(Some(selected - 1));
            } else {
                self.language_list_state
                    .select(Some(self.available_languages.len() - 1));
            }
        } else {
            self.language_list_state.select(Some(0));
        }
    }

    fn scroll_language_menu_down(&mut self) {
        if let Some(selected) = self.language_list_state.selected() {
            if selected < self.available_languages.len() - 1 {
                self.language_list_state.select(Some(selected + 1));
            } else {
                self.language_list_state.select(Some(0));
            }
        } else {
            self.language_list_state.select(Some(0));
        }
    }

    fn scroll_alto_menu_up(&mut self) {
        if let Some(selected) = self.alto_version_list_state.selected() {
            if selected > 0 {
                self.alto_version_list_state.select(Some(selected - 1));
            } else {
                self.alto_version_list_state
                    .select(Some(self.available_alto_versions.len() - 1));
            }
        } else {
            self.alto_version_list_state.select(Some(0));
        }
    }

    fn scroll_alto_menu_down(&mut self) {
        if let Some(selected) = self.alto_version_list_state.selected() {
            if selected < self.available_alto_versions.len() - 1 {
                self.alto_version_list_state.select(Some(selected + 1));
            } else {
                self.alto_version_list_state.select(Some(0));
            }
        } else {
            self.alto_version_list_state.select(Some(0));
        }
    }

    fn auto_scroll_log(&mut self) {
        let visible_lines = 20; // přibližný počet viditelných řádků
        let total_lines = self.log_lines.len();
        if total_lines > visible_lines {
            self.log_scroll = total_lines - visible_lines;
        }
    }

    fn next_focus(&mut self) {
        self.focus = match self.focus {
            FocusedPane::Jobs => FocusedPane::Detail,
            FocusedPane::Detail => FocusedPane::Log,
            FocusedPane::Log => FocusedPane::Jobs,
        };
    }

    fn prev_focus(&mut self) {
        self.focus = match self.focus {
            FocusedPane::Jobs => FocusedPane::Log,
            FocusedPane::Detail => FocusedPane::Jobs,
            FocusedPane::Log => FocusedPane::Detail,
        };
    }

    fn toggle_master(&mut self) {
        self.do_master = !self.do_master;
        self.push_log(format!(
            "AC (Master) {}",
            if self.do_master { "ON" } else { "OFF" }
        ));
        // Překontrolovat stav dávek po změně formátů
        self.update_jobs_status();
    }

    fn toggle_user(&mut self) {
        self.do_user = !self.do_user;
        self.push_log(format!(
            "UC (User) {}",
            if self.do_user { "ON" } else { "OFF" }
        ));
        // Překontrolovat stav dávek po změně formátů
        self.update_jobs_status();
    }

    fn toggle_txt(&mut self) {
        self.do_txt = !self.do_txt;
        self.push_log(format!("TXT {}", if self.do_txt { "ON" } else { "OFF" }));
        // Překontrolovat stav dávek po změně formátů
        self.update_jobs_status();
    }

    fn toggle_alto(&mut self) {
        self.do_alto = !self.do_alto;
        self.push_log(format!(
            "ALTO {} (verze {})",
            if self.do_alto { "ON" } else { "OFF" },
            self.args.alto_version
        ));
        // Překontrolovat stav dávek po změně formátů
        self.update_jobs_status();
    }

    fn update_jobs_status(&mut self) {
        update_jobs_status_on_start(
            &mut self.jobs,
            &self.output_root,
            &self.input_root,
            self.do_master,
            self.do_user,
            self.do_txt,
            self.do_alto,
            self.args.digits,
        );
    }

    fn start_edit_input(&mut self) {
        self.mode = UiMode::EditInput;
        self.edit_buffer = self.input_root.to_string_lossy().to_string();
        self.push_log(
            "Editace input root adresáře – Enter=potvrdit, Esc=zrušit".to_string(),
        );
    }

    fn start_edit_output(&mut self) {
        self.mode = UiMode::EditOutput;
        self.edit_buffer = self.output_root.to_string_lossy().to_string();
        self.push_log("Editace output root – Enter=potvrdit, Esc=zrušit".to_string());
    }

    fn show_language_menu(&mut self) {
        self.mode = UiMode::LanguageMenu;
        self.push_log(
            "Výběr jazyka – ↑/↓: pohyb, Enter: vybrat, F2: vlastní zadání, Esc: zrušit"
                .to_string(),
        );
    }

    fn show_alto_version_menu(&mut self) {
        self.mode = UiMode::AltoVersionMenu;
        self.push_log("Výběr ALTO verze – ↑/↓: pohyb, Enter: vybrat, Esc: zrušit".to_string());
    }

    fn start_custom_lang_input(&mut self) {
        self.mode = UiMode::CustomLangInput;
        self.custom_lang_input = self.args.lang.clone();
        self.push_log(
            "Vlastní kombinace jazyků (např. eng+ces+deu) – Enter=potvrdit, Esc=zrušit"
                .to_string(),
        );
    }

    fn apply_edit(&mut self) {
        let trimmed = self.edit_buffer.trim().to_string();
        match self.mode {
            UiMode::EditInput => {
                let new_path = PathBuf::from(&trimmed);
                if !new_path.is_dir() {
                    self.push_log(format!(
                        "Input root adresář `{}` neexistuje nebo není adresář.",
                        new_path.display()
                    ));
                } else {
                    self.input_root = new_path.clone();
                    self.push_log(format!("Input root nastaven na `{}`", new_path.display()));
                    let jobs =
                        init_jobs_from_dirs(&self.input_root, self.args.start_index)
                            .unwrap_or_else(|e| {
                                self.push_log(format!(
                                    "Chyba při načítání dávek: {e}. Joby zůstávají prázdné."
                                ));
                                Vec::new()
                            });
                    self.jobs = jobs;
                    self.selected = 0;
                    self.detail_scroll = 0;
                    self.log_scroll = 0;
                    // Kontrola stavu nových dávek
                    self.update_jobs_status();
                }
            }
            UiMode::EditOutput => {
                let new_path = PathBuf::from(&trimmed);
                if let Err(e) = fs::create_dir_all(&new_path) {
                    self.push_log(format!(
                        "Nelze vytvořit output root `{}`: {e}",
                        new_path.display()
                    ));
                } else {
                    self.output_root = new_path.clone();
                    self.push_log(format!("Output root nastaven na `{}`", new_path.display()));
                    // Kontrola stavu dávek s novou output cestou
                    self.update_jobs_status();
                }
            }
            UiMode::Normal => {}
            _ => {}
        }
        self.mode = UiMode::Normal;
        self.edit_buffer.clear();
    }

    fn cancel_edit(&mut self) {
        self.mode = UiMode::Normal;
        self.edit_buffer.clear();
        self.custom_lang_input.clear();
        self.push_log("Editace zrušena.".to_string());
    }

    fn handle_edit_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc => self.cancel_edit(),
            KeyCode::Enter => self.apply_edit(),
            KeyCode::Backspace => {
                self.edit_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.edit_buffer.push(c);
            }
            _ => {}
        }
    }

    fn handle_custom_lang_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc => {
                self.mode = UiMode::LanguageMenu;
                self.custom_lang_input.clear();
                self.push_log("Vlastní zadání jazyka zrušeno.".to_string());
            }
            KeyCode::Enter => {
                let trimmed = self.custom_lang_input.trim().to_string();
                if !trimmed.is_empty() {
                    self.args.lang = trimmed;
                    self.push_log(format!(
                        "Jazyk nastaven na vlastní kombinaci: {}",
                        self.args.lang
                    ));
                    self.mode = UiMode::Normal;
                    self.custom_lang_input.clear();
                }
            }
            KeyCode::Backspace => {
                self.custom_lang_input.pop();
            }
            KeyCode::Char(c) => {
                self.custom_lang_input.push(c);
            }
            _ => {}
        }
    }

    fn handle_language_menu_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc => {
                self.mode = UiMode::Normal;
                self.push_log("Výběr jazyka zrušen.".to_string());
            }
            KeyCode::Up => {
                self.scroll_language_menu_up();
            }
            KeyCode::Down => {
                self.scroll_language_menu_down();
            }
            KeyCode::PageUp => {
                for _ in 0..5 {
                    self.scroll_language_menu_up();
                }
            }
            KeyCode::PageDown => {
                for _ in 0..5 {
                    self.scroll_language_menu_down();
                }
            }
            KeyCode::F(2) => {
                self.start_custom_lang_input();
            }
            KeyCode::Enter => {
                if let Some(selected) = self.language_list_state.selected() {
                    if selected < self.available_languages.len() {
                        self.args.lang = self.available_languages[selected].0.clone();
                        self.push_log(format!("Jazyk nastaven na: {}", self.args.lang));
                        self.mode = UiMode::Normal;
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_alto_version_menu_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc => {
                self.mode = UiMode::Normal;
                self.push_log("Výběr ALTO verze zrušen.".to_string());
            }
            KeyCode::Up => {
                self.scroll_alto_menu_up();
            }
            KeyCode::Down => {
                self.scroll_alto_menu_down();
            }
            KeyCode::PageUp => {
                for _ in 0..5 {
                    self.scroll_alto_menu_up();
                }
            }
            KeyCode::PageDown => {
                for _ in 0..5 {
                    self.scroll_alto_menu_down();
                }
            }
            KeyCode::Enter => {
                if let Some(selected) = self.alto_version_list_state.selected() {
                    if selected < self.available_alto_versions.len() {
                        self.args.alto_version = self.available_alto_versions[selected].clone();
                        self.push_log(format!(
                            "ALTO verze nastavena na: {}",
                            self.args.alto_version
                        ));
                        self.mode = UiMode::Normal;
                    }
                }
            }
            _ => {}
        }
    }

    fn push_log(&mut self, line: String) {
        let timestamp = Local::now().format("[%H:%M:%S] ").to_string();
        self.log_lines.push(format!("{}{}", timestamp, line));
        if self.log_lines.len() > 1000 {
            let extra = self.log_lines.len() - 500;
            self.log_lines.drain(0..extra);
        }
        // Auto-scroll pouze pokud uživatel není moc vysoko
        if self.log_scroll >= self.log_lines.len().saturating_sub(30) {
            self.auto_scroll_log();
        }
    }

    fn process_selected(&mut self) {
        if self.jobs.is_empty() {
            self.push_log("Žádná dávka k zpracování.".to_string());
            return;
        }
        
        let idx = self.selected;
        let status = &self.jobs[idx].status;
        
        match status {
            JobStatus::AlreadyDone => {
                self.push_log("Dávka je již zpracována. Použijte 'F' pro vynucení přepracování.".to_string());
            }
            _ => {
                self.run_job(idx);
            }
        }
    }

    fn process_all_pending(&mut self) {
        if self.jobs.is_empty() {
            self.push_log("Žádné dávky k zpracování.".to_string());
            return;
        }
        
        self.push_log("Spouštím zpracování všech Pending dávek...".to_string());
        
        for i in 0..self.jobs.len() {
            let status = &self.jobs[i].status;
            let is_processable = matches!(status, JobStatus::Pending | JobStatus::Failed(_));
            
            if is_processable {
                self.run_job(i);
            }
        }
        
        self.push_log("Hromadné zpracování dokončeno.".to_string());
    }

    fn force_rerun_selected(&mut self) {
        if self.jobs.is_empty() {
            self.push_log("Žádná dávka k přepracování.".to_string());
            return;
        }
        
        let idx = self.selected;
        let current_status = &self.jobs[idx].status;
        
        match current_status {
            JobStatus::AlreadyDone | JobStatus::Done => {
                self.jobs[idx].status = JobStatus::Pending;
                self.push_log(format!("Dávka {} nastavena na Pending pro přepracování.", idx));
            }
            JobStatus::Processing => {
                self.push_log("Dávka právě zpracovává - nelze přepracovat.".to_string());
            }
            _ => {
                self.push_log("Dávka již je ve stavu Pending nebo Failed.".to_string());
            }
        }
    }
    
    fn force_rerun_all(&mut self) {
        let mut count = 0;
        for job in self.jobs.iter_mut() {
            if matches!(job.status, JobStatus::AlreadyDone | JobStatus::Done) {
                job.status = JobStatus::Pending;
                count += 1;
            }
        }
        self.push_log(format!("{} hotových/AlreadyDone dávek nastaveno na Pending.", count));
    }

    fn run_job(&mut self, job_index: usize) {
        if job_index >= self.jobs.len() {
            return;
        }

        let dir;
        let index_start;
        let file_count;
        {
            let job = &mut self.jobs[job_index];
            job.status = JobStatus::Processing;
            dir = job.dir.clone();
            index_start = job.index_start;
            file_count = job.file_count;
        }

        let args = self.args.clone();
        let do_master = self.do_master;
        let do_user = self.do_user;
        let do_txt = self.do_txt;
        let do_alto = self.do_alto;
        let output_root = self.output_root.clone();
        let input_root = self.input_root.clone();
        let grok_path = self.grok_path.clone();
        let tess_path = self.tess_path.clone();
        let alto_version = self.args.alto_version.clone();
        let tessdata_dir = self.tessdata_dir.clone();

        let batch_out_dir = batch_output_dir(&output_root, &input_root, &dir);
        let _ = fs::create_dir_all(&batch_out_dir);

        let mut local_logs: Vec<String> = Vec::new();
        local_logs.push(format!(
            "=== Dávka {} ({}), start index {}, ALTO v{} ===",
            dir.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<root>"),
            dir.display(),
            index_start,
            alto_version
        ));
        
        // Přidáme informace o použitém Tesseractu
        local_logs.push(format!("Tesseract: {} ({})", 
            self.tess_path.display(), 
            self.tess_source));
        local_logs.push(format!("Tessdata dir: {:?}", tessdata_dir));

        // 1. Nejprve zpracujeme dávku (JP2, OCR, ALTO)
        let res = process_batch(
            &args,
            &grok_path,
            &tess_path,
            &dir,
            index_start,
            &batch_out_dir,
            do_master,
            do_user,
            do_txt,
            do_alto,
            &alto_version,
            tessdata_dir.as_deref(),
            &mut local_logs,
        );

        let batch_successful = res.is_ok();
        
        if batch_successful {
            if let Some(job) = self.jobs.get_mut(job_index) {
                job.status = JobStatus::Done;
            }
            local_logs.push("Dávka OK".to_string());
            
            // Malé zpoždění pro zápis souborů na disk
            local_logs.push("Čekám na dokončení zápisu souborů...".to_string());
            std::thread::sleep(std::time::Duration::from_millis(1000));
            
            // 2. Počkáme na vytvoření JP2 souborů (pokud jsou povoleny)
            if do_master || do_user {
                wait_for_jp2_files(&batch_out_dir, index_start, file_count, 
                    args.digits, do_master, do_user, &mut local_logs);
            }
        } else {
            if let Some(job) = self.jobs.get_mut(job_index) {
                job.status = JobStatus::Failed(res.err().unwrap().to_string());
            }
            local_logs.push("Dávka FAILED".to_string());
        }

        // 3. Teprve po úspěšném zpracování dávky pokračujeme s manifestem a WebP
        if batch_successful {
            let batch_name = dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("root")
                .to_string();
            let safe_batch = if batch_name.is_empty() {
                "root".to_string()
            } else {
                batch_name.clone()
            };
            let logs_dir = self.output_root.join(format!("{safe_batch}_logs"));

            // 4. Vytvoříme manifest (teprve po vytvoření všech souborů)
            match build_manifest_for_batch(
                &safe_batch,
                &dir,
                &batch_out_dir,
                &logs_dir,
                index_start,
                args.digits,
                do_master,
                do_user,
                do_txt,
                do_alto,
                &args.lang,
                &alto_version,
            ) {
                Ok(manifest) => {
                    // 5. Zapišeme manifest a log.txt - s přidáním logu
                    if let Err(e) = write_manifest_and_log_with_process_logs(&manifest, &logs_dir, &local_logs) {
                        local_logs.push(format!("Chyba při zápisu manifestu/log.txt: {e}"));
                    } else {
                        local_logs.push("Manifest a log.txt vytvořeny".to_string());
                    }

                    // 6. Zkontrolujeme, zda máme co konvertovat na WebP
                    let has_jp2_files = manifest.pages.iter().any(|p| p.ac_jp2.is_some() || p.uc_jp2.is_some());
                    
                    if has_jp2_files {
                        // 7. WebP náhledy (teprve po vytvoření JP2 a manifestu)
                        local_logs.push("Generuji WebP náhledy...".to_string());
                        match generate_webp_previews(&logs_dir) {
                            Ok(()) => {
                                local_logs.push(format!(
                                    "WEBP náhledy vygenerovány v {} (z TIFF)",
                                    logs_dir.display()
                                ));
                            }
                            Err(e) => {
                                local_logs.push(format!(
                                    "Chyba při generování WEBP náhledů: {e}"
                                ));
                            }
                        }
                    } else {
                        local_logs.push("Žádné JP2 soubory pro WebP konverzi".to_string());
                    }

                    // 8. HTML report (až po WebP)
                    match write_html_report(&logs_dir) {
                        Ok(()) => {
                            local_logs.push(format!(
                                "HTML report vytvořen: {}",
                                logs_dir.join("index.html").display()
                            ));
                        }
                        Err(e) => {
                            local_logs.push(format!(
                                "Chyba při tvorbě HTML reportu: {e}"
                            ));
                        }
                    }
                }
                Err(e) => {
                    local_logs.push(format!("Chyba při generování manifestu: {e}"));
                }
            }
        }

        for line in local_logs {
            self.push_log(line);
        }
    }
}

/// Zkontroluje, zda byla dávka již kompletně zpracována
fn check_batch_already_done(
    batch: &BatchJob,
    output_root: &Path,
    input_root: &Path,
    do_master: bool,
    do_user: bool,
    do_txt: bool,
    do_alto: bool,
    digits: usize,
) -> bool {
    let batch_out_dir = batch_output_dir(output_root, input_root, &batch.dir);
    
    // Pokud výstupní adresář neexistuje, dávka určitě nebyla zpracována
    if !batch_out_dir.exists() {
        return false;
    }
    
    let mut required_formats = 0;
    
    // Spočítáme, kolik formátů má být vygenerováno
    if do_master { required_formats += 1; }
    if do_user { required_formats += 1; }
    if do_txt { required_formats += 1; }
    if do_alto { required_formats += 1; }
    
    // Pokud nic nemá být vygenerováno, považujeme za hotové
    if required_formats == 0 {
        return true;
    }
    
    // Pro každý soubor v dávce kontrolujeme
    for i in 0..batch.file_count {
        let idx = batch.index_start + i as u32;
        let index_str = format!("{:0digits$}", idx);
        
        let mut file_formats = 0;
        
        if do_master {
            let jp2_path = batch_out_dir.join(format!("{index_str}.ac.jp2"));
            if jp2_path.exists() { file_formats += 1; }
        }
        
        if do_user {
            let jp2_path = batch_out_dir.join(format!("{index_str}.uc.jp2"));
            if jp2_path.exists() { file_formats += 1; }
        }
        
        if do_txt {
            let txt_path = batch_out_dir.join(format!("{index_str}.ocr.txt"));
            if txt_path.exists() { file_formats += 1; }
        }
        
        if do_alto {
            let alto_path = batch_out_dir.join(format!("{index_str}.ocr.xml"));
            if alto_path.exists() { file_formats += 1; }
        }
        
        // Pokud nějaký soubor nemá všechny požadované formáty, dávka není kompletní
        if file_formats < required_formats {
            return false;
        }
    }
    
    true
}

/// Aktualizace stavu všech dávek při startu nebo změně formátů
fn update_jobs_status_on_start(
    jobs: &mut [BatchJob],
    output_root: &Path,
    input_root: &Path,
    do_master: bool,
    do_user: bool,
    do_txt: bool,
    do_alto: bool,
    digits: usize,
) {
    for job in jobs.iter_mut() {
        if check_batch_already_done(
            job, output_root, input_root, 
            do_master, do_user, do_txt, do_alto, digits
        ) {
            // Pokud je již Done, zachováme to, jinak nastavíme AlreadyDone
            if !matches!(job.status, JobStatus::Done) {
                job.status = JobStatus::AlreadyDone;
            }
        } else if matches!(job.status, JobStatus::AlreadyDone) {
            // Pokud kontrola neprojde a stav je AlreadyDone, změníme na Pending
            job.status = JobStatus::Pending;
        }
    }
}

/// Počká na vytvoření JP2 souborů
fn wait_for_jp2_files(
    output_dir: &Path,
    index_start: u32,
    file_count: usize,
    digits: usize,
    do_master: bool,
    do_user: bool,
    logs: &mut Vec<String>,
) {
    let max_attempts = 5;
    let delay = std::time::Duration::from_millis(500);
    
    for attempt in 1..=max_attempts {
        let mut all_files_exist = true;
        
        for i in 0..file_count {
            let idx = index_start + i as u32;
            let index_str = format!("{idx:0digits$}");
            
            if do_master {
                let jp2_path = output_dir.join(format!("{index_str}.ac.jp2"));
                if !jp2_path.exists() {
                    all_files_exist = false;
                    break;
                }
            }
            
            if do_user {
                let jp2_path = output_dir.join(format!("{index_str}.uc.jp2"));
                if !jp2_path.exists() {
                    all_files_exist = false;
                    break;
                }
            }
        }
        
        if all_files_exist {
            logs.push(format!("Všechny JP2 soubory připraveny (pokus {}/{})", attempt, max_attempts));
            return;
        }
        
        if attempt < max_attempts {
            logs.push(format!("Čekám na JP2 soubory... (pokus {}/{})", attempt, max_attempts));
            std::thread::sleep(delay);
        }
    }
    
    logs.push("Varování: Některé JP2 soubory nebyly vytvořeny včas".to_string());
}

fn main() -> Result<()> {
    let args = Args::parse();

    // defaultní root / output adresáře v rootu programu
    let input_root = args.input.clone().unwrap_or_else(|| PathBuf::from("input"));
    let output_root = args.output.clone().unwrap_or_else(|| PathBuf::from("output"));

    // output root vždy vytvořit
    fs::create_dir_all(&output_root).with_context(|| {
        format!(
            "Nelze vytvořit výstupní kořenový adresář `{}`",
            output_root.display()
        )
    })?;

    // input root vytvoříme taky, pokud neexistuje
    if !input_root.exists() {
        fs::create_dir_all(&input_root).with_context(|| {
            format!(
                "Nelze vytvořit vstupní adresář `{}`",
                input_root.display()
            )
        })?;
    }

    // zjištění Grok cesty + status + "verze"
    let grok_path = resolve_grok_path(&args.grok_bin);
    let (grok_status, grok_version) = check_grok(&grok_path, args.dry_run);

    // zjištění Tesseract cesty + status s prioritou lokálního
    let (tess_path, tess_source) = resolve_tess_path_with_priority(&args.tess_bin, args.force_local_tess);
    let tess_status = check_tesseract(&tess_path, args.dry_run);
    
    // Najít nebo použít zadaný tessdata adresář
    let tessdata_dir = if let Some(ref td) = args.tessdata_dir {
        Some(td.clone())
    } else {
        find_tessdata_parent_dir(&tess_path)
    };

    let jobs = init_jobs_from_dirs(&input_root, args.start_index)?;

    let mut app = App::new(
        args,
        input_root,
        output_root,
        jobs,
        grok_path,
        grok_status,
        grok_version,
        tess_path,
        tess_source,
        tess_status,
        tessdata_dir,
    );

    // Terminál
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("Error: {e:?}");
    }

    Ok(())
}

/// Spočítá cílový adresář pro dávku:
/// - pokud dávka je přímo input_root → output_root
/// - jinak output_root / <název dávky>
fn batch_output_dir(output_root: &Path, input_root: &Path, batch_dir: &Path) -> PathBuf {
    if batch_dir == input_root {
        output_root.to_path_buf()
    } else {
        match batch_dir.file_name() {
            Some(name) => output_root.join(name),
            None => output_root.to_path_buf(),
        }
    }
}

/// Inicializace dávek podle adresářů v input_root:
/// - dávka = samotný input_root (pokud obsahuje TIFFy)
/// - + každý podadresář (jen 1. úroveň) s nějakými TIFFy
/// Indexy stránek běží sekvenčně přes všechny dávky.
fn init_jobs_from_dirs(input_root: &Path, start_index: u32) -> Result<Vec<BatchJob>> {
    let mut batch_dirs: Vec<PathBuf> = Vec::new();

    // nejdřív samotný root, pokud obsahuje TIFFy
    if has_tiffs_in_dir(input_root)? {
        batch_dirs.push(input_root.to_path_buf());
    }

    // potom první úroveň podadresářů
    if input_root.is_dir() {
        for entry in fs::read_dir(input_root)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && has_tiffs_in_dir(&path)? {
                batch_dirs.push(path);
            }
        }
    }

    // seřadíme podle názvu pro deterministické pořadí
    batch_dirs.sort_by(|a, b| {
        let a_name = a.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let b_name = b.file_name().and_then(|s| s.to_str()).unwrap_or("");
        a_name.cmp(b_name)
    });

    let mut jobs = Vec::new();
    let mut next_index = start_index;

    for dir in batch_dirs {
        let files = collect_tiffs_in_dir(&dir)?;
        let count = files.len();
        if count == 0 {
            continue;
        }
        jobs.push(BatchJob {
            dir: dir.clone(),
            index_start: next_index,
            file_count: count,
            status: JobStatus::Pending,
        });
        next_index += count as u32;
    }

    Ok(jobs)
}

/// Vrátí true, pokud v daném adresáři jsou nějaké TIFFy (pouze 1. úroveň, ne rekurzivně).
fn has_tiffs_in_dir(dir: &Path) -> Result<bool> {
    if !dir.is_dir() {
        return Ok(false);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "tif" || ext_lower == "tiff" {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// Najde všechny .tif / .tiff soubory v daném adresáři (pouze 1. úroveň, ne rekurzivně).
pub fn collect_tiffs_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "tif" || ext_lower == "tiff" {
                    out.push(path);
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Hledá Tesseract v PATH (bez which crate)
fn find_tesseract_in_path() -> Option<PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        let separator = if cfg!(windows) { ';' } else { ':' };
        let paths: Vec<&str> = path_var.split(separator).collect();
        
        for path in paths {
            let mut test_path = PathBuf::from(path);
            if cfg!(windows) {
                test_path = test_path.join("tesseract.exe");
            } else {
                test_path = test_path.join("tesseract");
            }
            
            if test_path.exists() {
                return Some(test_path);
            }
        }
    }
    None
}

/// Hlavní TUI smyčka
fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(200);

    // Proměnná pro sledování změn, které vyžadují kompletní reset
    let mut force_full_redraw = true;

    loop {
        // Pokud potřebujeme kompletní překreslení, zavoláme clear()
        if force_full_redraw {
            terminal.clear()?;
            force_full_redraw = false;
        }
        
        terminal.draw(|f| ui(f, app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    let mut needs_full_redraw = false;
                    
                    match app.mode {
                        UiMode::Normal => match key.code {
                            KeyCode::Char('q') => return Ok(()),
                            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                                // Změna výběru nebo scrollování - vyžaduje kompletní překreslení
                                match key.code {
                                    KeyCode::Up => app.up(),
                                    KeyCode::Down => app.down(),
                                    KeyCode::PageUp => app.page_up(),
                                    KeyCode::PageDown => app.page_down(),
                                    _ => {}
                                }
                                needs_full_redraw = true;
                            }
                            KeyCode::Tab | KeyCode::BackTab => {
                                // Změna fokusu - vyžaduje kompletní překreslení
                                match key.code {
                                    KeyCode::Tab => app.next_focus(),
                                    KeyCode::BackTab => app.prev_focus(),
                                    _ => {}
                                }
                                needs_full_redraw = true;
                            }
                            KeyCode::Char('m') => { 
                                app.toggle_master();
                                needs_full_redraw = true;
                            }
                            KeyCode::Char('u') => { 
                                app.toggle_user();
                                needs_full_redraw = true;
                            }
                            KeyCode::Char('t') => { 
                                app.toggle_txt();
                                needs_full_redraw = true;
                            }
                            KeyCode::Char('l') => { 
                                app.toggle_alto();
                                needs_full_redraw = true;
                            }
                            KeyCode::Char('i') | KeyCode::Char('I') => { 
                                app.start_edit_input(); 
                                needs_full_redraw = true;
                            }
                            KeyCode::Char('o') | KeyCode::Char('O') => { 
                                app.start_edit_output(); 
                                needs_full_redraw = true;
                            }
                            KeyCode::Char('L') => { 
                                app.show_language_menu(); 
                                needs_full_redraw = true;
                            }
                            KeyCode::Char('A') => { 
                                app.show_alto_version_menu(); 
                                needs_full_redraw = true;
                            }
                            KeyCode::Enter => { 
                                app.process_selected(); 
                                needs_full_redraw = true;
                            }
                            KeyCode::Char('a') => { 
                                app.process_all_pending(); 
                                needs_full_redraw = true;
                            }
                            KeyCode::Char('F') => {
                                app.force_rerun_selected();
                                needs_full_redraw = true;
                            }
                            KeyCode::Char('R') => {
                                app.force_rerun_all();
                                needs_full_redraw = true;
                            }
                            _ => {}
                        },
                        UiMode::EditInput | UiMode::EditOutput => {
                            app.handle_edit_key(key.code);
                            needs_full_redraw = true;
                        }
                        UiMode::LanguageMenu => {
                            app.handle_language_menu_key(key.code);
                            needs_full_redraw = true;
                        }
                        UiMode::AltoVersionMenu => {
                            app.handle_alto_version_menu_key(key.code);
                            needs_full_redraw = true;
                        }
                        UiMode::CustomLangInput => {
                            app.handle_custom_lang_key(key.code);
                            needs_full_redraw = true;
                        }
                    }
                    
                    if needs_full_redraw {
                        force_full_redraw = true;
                    }
                }
                Event::Mouse(me) => match me.kind {
                    MouseEventKind::ScrollUp => {
                        app.scroll_current_up(3);
                        force_full_redraw = true;
                    }
                    MouseEventKind::ScrollDown => {
                        app.scroll_current_down(3);
                        force_full_redraw = true;
                    }
                    _ => {}
                },
                Event::Resize(_, _) => {
                    // Změna velikosti okna vždy vyžaduje kompletní překreslení
                    force_full_redraw = true;
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
}

/// UI layout
fn ui(f: &mut Frame<'_>, app: &App) {
    // Nejprve hlavní UI
    render_main_ui(f, app);
    // Potom overlay (menu / inputy)
    render_overlay_ui(f, app);
}

fn render_main_ui(f: &mut Frame<'_>, app: &App) {
    let area = f.size();

    // Pozadí
    let background_block = Block::default().style(Style::default().bg(Color::Black));
    f.render_widget(background_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(0)
        .constraints(
            [
                Constraint::Length(4),      // hlavička + nástroje
                Constraint::Percentage(45), // seznam dávek
                Constraint::Percentage(20), // detail
                Constraint::Length(1),      // status bar
                Constraint::Percentage(30), // log
            ]
            .as_ref(),
        )
        .split(area);

    // ----- horní panel -----
    let title_line = Line::from(vec![
        Span::styled(
            "LazyProArcConvert",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" • "),
        Span::styled(
            "JP2+OCR/ALTO batch",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ),
    ]);

    let grok_status_text = match &app.grok_status {
        ToolStatus::Ok(msg) => Span::styled(
            format!("✓ {}", msg),
            Style::default().fg(Color::Green),
        ),
        ToolStatus::Error(msg) => Span::styled(
            format!("✗ {}", msg),
            Style::default().fg(Color::Red),
        ),
    };

    let tess_status_text = match &app.tess_status {
        ToolStatus::Ok(msg) => Span::styled(
            format!("✓ {}", msg),
            Style::default().fg(Color::Green),
        ),
        ToolStatus::Error(msg) => Span::styled(
            format!("✗ {}", msg),
            Style::default().fg(Color::Red),
        ),
    };

    let status_line = Line::from(vec![
        Span::raw("  "),
        Span::styled("Grok: ", Style::default().fg(Color::Cyan)),
        grok_status_text,
        Span::raw("  "),
        Span::styled("Tess: ", Style::default().fg(Color::Cyan)),
        tess_status_text,
        Span::raw("  "),
        Span::styled("Jazyk: ", Style::default().fg(Color::Cyan)),
        Span::raw(&app.args.lang),
        Span::raw("  "),
        Span::styled("ALTO: ", Style::default().fg(Color::Cyan)),
        Span::raw(&app.args.alto_version),
    ]);

    let mode_text = match app.mode {
        UiMode::Normal => Span::raw("Normal"),
        UiMode::EditInput => Span::styled("Editace input", Style::default().fg(Color::Yellow)),
        UiMode::EditOutput => Span::styled("Editace output", Style::default().fg(Color::Yellow)),
        UiMode::LanguageMenu => Span::styled("Výběr jazyka", Style::default().fg(Color::Yellow)),
        UiMode::AltoVersionMenu => Span::styled("Výběr ALTO", Style::default().fg(Color::Yellow)),
        UiMode::CustomLangInput => {
            Span::styled("Vlastní jazyk", Style::default().fg(Color::Yellow))
        }
    };

    let mode_line = Line::from(vec![
        Span::styled("Režim: ", Style::default().fg(Color::Yellow)),
        mode_text,
    ]);

    let status_block = Block::default()
        .title(Span::styled(" Status ", Style::default().fg(Color::Cyan)))
        .borders(Borders::ALL)
        .border_style(if matches!(app.mode, UiMode::Normal) {
            Style::default()
        } else {
            Style::default().fg(Color::Yellow)
        });

    let status_paragraph = Paragraph::new(vec![title_line, status_line, mode_line])
        .alignment(Alignment::Left)
        .block(status_block);

    f.render_widget(status_paragraph, chunks[0]);

    // ----- panel – seznam dávek -----
    let jobs_title_style = if matches!(app.focus, FocusedPane::Jobs) {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    };

    let items: Vec<ListItem> = app
        .jobs
        .iter()
        .enumerate()
        .map(|(i, batch)| {
            let batch_name = batch
                .dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<root>");

            let (status_icon, status_style) = match &batch.status {
                JobStatus::Pending => ("○", Style::default().fg(Color::DarkGray)),
                JobStatus::Processing => (
                    "↻",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                JobStatus::Done => (
                    "✓",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                JobStatus::AlreadyDone => (
                    "✓",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ),
                JobStatus::Failed(_) => (
                    "✗",
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                ),
            };

            let mut spans = Vec::new();
            spans.push(Span::styled(
                if i == app.selected { "▶ " } else { "  " },
                if i == app.selected {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                },
            ));

            spans.push(Span::styled(
                format!("{:3}× ", batch.file_count),
                Style::default().fg(Color::Cyan),
            ));
            spans.push(Span::styled(status_icon, status_style));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("{:30}", batch_name.chars().take(30).collect::<String>()),
                Style::default().fg(Color::White),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!(
                    "[{:04}-{:04}]",
                    batch.index_start,
                    batch.index_start + batch.file_count as u32 - 1
                ),
                Style::default().fg(Color::DarkGray),
            ));

            ListItem::new(Line::from(spans))
        })
        .collect();

    let jobs_block = Block::default()
        .title(Span::styled(" Dávky ", jobs_title_style))
        .borders(Borders::ALL)
        .border_style(if matches!(app.focus, FocusedPane::Jobs) {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });

    let list = List::new(items).block(jobs_block);
    f.render_widget(list, chunks[1]);

    // ----- detail panel -----
    let detail_title_style = if matches!(app.focus, FocusedPane::Detail) {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    };

    let mut detail_lines = Vec::new();

    if !app.jobs.is_empty() {
        let batch = &app.jobs[app.selected];
        let batch_name = batch
            .dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<root>");

        detail_lines.push(Line::from(vec![
            Span::styled("Dávka: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                batch_name,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        let status_text = match &batch.status {
            JobStatus::Pending => Span::styled("Čeká", Style::default().fg(Color::DarkGray)),
            JobStatus::Processing => {
                Span::styled("Zpracovává se", Style::default().fg(Color::Yellow))
            }
            JobStatus::Done => Span::styled("Hotovo", Style::default().fg(Color::Green)),
            JobStatus::AlreadyDone => Span::styled("Již hotovo", Style::default().fg(Color::Blue)),
            JobStatus::Failed(e) => {
                Span::styled(format!("Chyba: {}", e), Style::default().fg(Color::Red))
            }
        };

        detail_lines.push(Line::from(vec![
            Span::styled("Stav: ", Style::default().fg(Color::Cyan)),
            status_text,
        ]));

        detail_lines.push(Line::from(vec![
            Span::styled("Soubory: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                batch.file_count.to_string(),
                Style::default().fg(Color::White),
            ),
            Span::raw(" • "),
            Span::styled("Indexy: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!(
                    "{}-{}",
                    batch.index_start,
                    batch.index_start + batch.file_count as u32 - 1
                ),
                Style::default().fg(Color::White),
            ),
        ]));
    } else {
        detail_lines.push(Line::from("Žádné dávky"));
    }

    detail_lines.push(Line::from(""));

    detail_lines.push(Line::from(vec![
        Span::styled("AC:", Style::default().fg(Color::Cyan)),
        Span::raw(" "),
        Span::styled(
            bool_label(app.do_master),
            Style::default().fg(if app.do_master {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw("  "),
        Span::styled("UC:", Style::default().fg(Color::Cyan)),
        Span::raw(" "),
        Span::styled(
            bool_label(app.do_user),
            Style::default().fg(if app.do_user {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw("  "),
        Span::styled("TXT:", Style::default().fg(Color::Cyan)),
        Span::raw(" "),
        Span::styled(
            bool_label(app.do_txt),
            Style::default().fg(if app.do_txt {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw("  "),
        Span::styled("ALTO:", Style::default().fg(Color::Cyan)),
        Span::raw(" "),
        Span::styled(
            bool_label(app.do_alto),
            Style::default().fg(if app.do_alto {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
    ]));

    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(vec![Span::styled(
        "OVLÁDÁNÍ:",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )]));
    detail_lines.push(Line::from(vec![
        Span::styled("m/u/t/l", Style::default().fg(Color::Yellow)),
        Span::raw(": přepínání formátů  "),
        Span::styled("L", Style::default().fg(Color::Yellow)),
        Span::raw("/"),
        Span::styled("A", Style::default().fg(Color::Yellow)),
        Span::raw(": jazyk/ALTO"),
    ]));
    detail_lines.push(Line::from(vec![
        Span::styled("i", Style::default().fg(Color::Yellow)),
        Span::raw("/"),
        Span::styled("o", Style::default().fg(Color::Yellow)),
        Span::raw(": editace input/output  "),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(": spustit dávku"),
    ]));
    detail_lines.push(Line::from(vec![
        Span::styled("F", Style::default().fg(Color::Yellow)),
        Span::raw(": vynutit přepracování  "),
        Span::styled("R", Style::default().fg(Color::Yellow)),
        Span::raw(": vynutit přepracování všech"),
    ]));
    detail_lines.push(Line::from(vec![
        Span::styled("a", Style::default().fg(Color::Yellow)),
        Span::raw(": všechny čekající  "),
        Span::styled("Tab", Style::default().fg(Color::Yellow)),
        Span::raw(": přepnout fokus"),
    ]));
    detail_lines.push(Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
        Span::raw(": výběr  "),
        Span::styled("PgUp/PgDn", Style::default().fg(Color::Yellow)),
        Span::raw(": stránkování"),
    ]));

    let detail_block = Block::default()
        .title(Span::styled(" Detail ", detail_title_style))
        .borders(Borders::ALL)
        .border_style(if matches!(app.focus, FocusedPane::Detail) {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });

    let detail_paragraph = Paragraph::new(detail_lines)
        .block(detail_block)
        .scroll((app.detail_scroll as u16, 0));

    f.render_widget(detail_paragraph, chunks[2]);

    // ----- status bar -----
    let total_batches = app.jobs.len();
    let pending_batches = app
        .jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::Pending))
        .count();
    let done_batches = app
        .jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::Done))
        .count();
    let already_done_batches = app
        .jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::AlreadyDone))
        .count();
    let failed_batches = app
        .jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::Failed(_)))
        .count();

    let total_pages: usize = app.jobs.iter().map(|j| j.file_count).sum();
    let done_pages: usize = app
        .jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::Done))
        .map(|j| j.file_count)
        .sum();
    let already_done_pages: usize = app
        .jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::AlreadyDone))
        .map(|j| j.file_count)
        .sum();

    let status_line = Line::from(vec![
        Span::styled("Dávky: ", Style::default().fg(Color::Cyan)),
        Span::styled(
            format!("{}/{} ", done_batches + already_done_batches, total_batches),
            Style::default().fg(if done_batches + already_done_batches == total_batches && total_batches > 0 {
                Color::Green
            } else {
                Color::White
            }),
        ),
        Span::raw("• "),
        Span::styled(
            format!("Čeká: {} ", pending_batches),
            Style::default().fg(if pending_batches > 0 {
                Color::Yellow
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw("• "),
        Span::styled(
            format!("Již hotovo: {} ", already_done_batches),
            Style::default().fg(if already_done_batches > 0 {
                Color::Blue
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw("• "),
        Span::styled(
            format!("Chyby: {} ", failed_batches),
            Style::default().fg(if failed_batches > 0 {
                Color::Red
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw("• "),
        Span::styled(
            format!("Stránky: {}/{} ", done_pages + already_done_pages, total_pages),
            Style::default().fg(if done_pages + already_done_pages == total_pages && total_pages > 0 {
                Color::Green
            } else {
                Color::White
            }),
        ),
        Span::raw("• "),
        Span::styled("AC:", Style::default().fg(Color::Cyan)),
        Span::styled(
            bool_label(app.do_master),
            Style::default().fg(if app.do_master {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw(" "),
        Span::styled("UC:", Style::default().fg(Color::Cyan)),
        Span::styled(
            bool_label(app.do_user),
            Style::default().fg(if app.do_user {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw(" "),
        Span::styled("TXT:", Style::default().fg(Color::Cyan)),
        Span::styled(
            bool_label(app.do_txt),
            Style::default().fg(if app.do_txt {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw(" "),
        Span::styled("ALTO:", Style::default().fg(Color::Cyan)),
        Span::styled(
            bool_label(app.do_alto),
            Style::default().fg(if app.do_alto {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ),
    ]);

    let status_bar = Paragraph::new(status_line).block(Block::default().borders(Borders::NONE));
    f.render_widget(status_bar, chunks[3]);

    // ----- log panel (ŘÁDKY BEZ BAREVNÉHO ZVÝRAZŇOVÁNÍ) -----
    let log_title_style = if matches!(app.focus, FocusedPane::Log) {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    };

    // prostý text – každá řádka jedna Line, bez analýzy obsahu
    let log_lines: Vec<Line> = app
        .log_lines
        .iter()
        .map(|line| Line::from(Span::raw(line.clone())))
        .collect();

    let log_block = Block::default()
        .title(Span::styled(
            format!(" Log ({}) ", app.log_lines.len()),
            log_title_style,
        ))
        .borders(Borders::ALL)
        .border_style(if matches!(app.focus, FocusedPane::Log) {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });

    let visible_log_height = chunks[4].height.saturating_sub(2);
    let total_log_lines = log_lines.len() as u16;
    let max_log_scroll = total_log_lines.saturating_sub(visible_log_height) as usize;
    let log_y = app.log_scroll.min(max_log_scroll) as u16;

    let logs = Paragraph::new(log_lines)
        .block(log_block)
        .scroll((log_y, 0));
    f.render_widget(logs, chunks[4]);
}

fn render_overlay_ui(f: &mut Frame<'_>, app: &App) {
    match app.mode {
        UiMode::LanguageMenu => {
            let area = centered_rect(60, 70, f.size());

            let background_block = Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Black));
            f.render_widget(background_block, area);

            let inner_area = Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(2),
            };

            let title = Line::from(vec![
                Span::styled(
                    " Výběr jazyka ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" (↑/↓ pohyb, Enter výběr, F2 vlastní kombinace, Esc zrušit)"),
            ]);

            let title_block = Block::default().title(title).borders(Borders::NONE);

            let items: Vec<ListItem> = app
                .available_languages
                .iter()
                .enumerate()
                .map(|(i, (code, name))| {
                    let is_selected = Some(i) == app.language_list_state.selected();
                    let is_current = code == &app.args.lang;

                    let mut spans = vec![];
                    if is_selected {
                        spans.push(Span::styled(
                            "▶ ",
                            Style::default().fg(Color::Yellow),
                        ));
                    } else {
                        spans.push(Span::raw("  "));
                    }

                    if is_current {
                        spans.push(Span::styled(
                            format!("{:15} ", code),
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ));
                        spans.push(Span::styled(
                            name,
                            Style::default().fg(Color::Green),
                        ));
                        spans.push(Span::raw(" (aktuální)"));
                    } else if is_selected {
                        spans.push(Span::styled(
                            format!("{:15} ", code),
                            Style::default().fg(Color::Yellow),
                        ));
                        spans.push(Span::styled(
                            name,
                            Style::default().fg(Color::Yellow),
                        ));
                    } else {
                        spans.push(Span::styled(
                            format!("{:15} ", code),
                            Style::default().fg(Color::White),
                        ));
                        spans.push(Span::styled(
                            name,
                            Style::default().fg(Color::Gray),
                        ));
                    }

                    ListItem::new(Line::from(spans))
                })
                .collect();

            let list = List::new(items)
                .block(title_block)
                .highlight_style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                );

            // DŮLEŽITÉ: použijeme skutečný state, ne clone
            let mut state = app.language_list_state.clone();
            f.render_stateful_widget(list, inner_area, &mut state);
            // `state` jen používáme pro scroll; skutečný výběr držíme v App
        }
        UiMode::AltoVersionMenu => {
            let area = centered_rect(40, 40, f.size());

            let background_block = Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Black));
            f.render_widget(background_block, area);

            let inner_area = Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(2),
            };

            let title = Line::from(vec![
                Span::styled(
                    " Výběr ALTO verze ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" (↑/↓ pohyb, Enter výběr, Esc zrušit)"),
            ]);

            let title_block = Block::default().title(title).borders(Borders::NONE);

            let items: Vec<ListItem> = app
                .available_alto_versions
                .iter()
                .enumerate()
                .map(|(i, ver)| {
                    let is_selected = Some(i) == app.alto_version_list_state.selected();
                    let is_current = ver == &app.args.alto_version;

                    let mut spans = vec![];
                    if is_selected {
                        spans.push(Span::styled(
                            "▶ ",
                            Style::default().fg(Color::Yellow),
                        ));
                    } else {
                        spans.push(Span::raw("  "));
                    }

                    if is_current {
                        spans.push(Span::styled(
                            ver,
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ));
                        spans.push(Span::raw(" (aktuální)"));
                    } else if is_selected {
                        spans.push(Span::styled(
                            ver,
                            Style::default().fg(Color::Yellow),
                        ));
                    } else {
                        spans.push(Span::styled(
                            ver,
                            Style::default().fg(Color::White),
                        ));
                    }

                    ListItem::new(Line::from(spans))
                })
                .collect();

            let list = List::new(items)
                .block(title_block)
                .highlight_style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                );

            let mut state = app.alto_version_list_state.clone();
            f.render_stateful_widget(list, inner_area, &mut state);
        }
        UiMode::EditInput => {
            let area = centered_rect(60, 20, f.size());

            let background_block = Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Black));
            f.render_widget(background_block, area);

            let inner_area = Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(2),
            };

            let edit_block = Block::default()
                .title(Span::styled(
                    " Editace input root ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::NONE);

            let edit_text = vec![
                Line::from("Zadejte cestu k input adresáři:"),
                Line::from(""),
                Line::from(vec![
                    Span::raw("> "),
                    Span::styled(
                        &app.edit_buffer,
                        Style::default().fg(Color::White),
                    ),
                    Span::styled("_", Style::default().fg(Color::Yellow)),
                ]),
                Line::from(""),
                Line::from("Enter: potvrdit, Esc: zrušit"),
            ];

            let edit_paragraph =
                Paragraph::new(edit_text).block(edit_block).alignment(Alignment::Left);

            f.render_widget(edit_paragraph, inner_area);
        }
        UiMode::EditOutput => {
            let area = centered_rect(60, 20, f.size());

            let background_block = Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Black));
            f.render_widget(background_block, area);

            let inner_area = Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(2),
            };

            let edit_block = Block::default()
                .title(Span::styled(
                    " Editace output root ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::NONE);

            let edit_text = vec![
                Line::from("Zadejte cestu k output adresáři:"),
                Line::from(""),
                Line::from(vec![
                    Span::raw("> "),
                    Span::styled(
                        &app.edit_buffer,
                        Style::default().fg(Color::White),
                    ),
                    Span::styled("_", Style::default().fg(Color::Yellow)),
                ]),
                Line::from(""),
                Line::from("Enter: potvrdit, Esc: zrušit"),
            ];

            let edit_paragraph =
                Paragraph::new(edit_text).block(edit_block).alignment(Alignment::Left);

            f.render_widget(edit_paragraph, inner_area);
        }
        UiMode::CustomLangInput => {
            let area = centered_rect(60, 20, f.size());

            let background_block = Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Black));
            f.render_widget(background_block, area);

            let inner_area = Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(2),
            };

            let edit_block = Block::default()
                .title(Span::styled(
                    " Vlastní kombinace jazyků ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::NONE);

            let edit_text = vec![
                Line::from("Zadejte kódy jazyků oddělené '+' (např. eng+ces+deu):"),
                Line::from(""),
                Line::from(vec![
                    Span::raw("> "),
                    Span::styled(
                        &app.custom_lang_input,
                        Style::default().fg(Color::White),
                    ),
                    Span::styled("_", Style::default().fg(Color::Yellow)),
                ]),
                Line::from(""),
                Line::from("Enter: potvrdit, Esc: zrušit"),
            ];

            let edit_paragraph =
                Paragraph::new(edit_text).block(edit_block).alignment(Alignment::Left);

            f.render_widget(edit_paragraph, inner_area);
        }
        UiMode::Normal => {}
    }
}

fn bool_label(b: bool) -> String {
    if b {
        "ON".to_string()
    } else {
        "OFF".to_string()
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Zpracuje jednu dávku = adresář.
fn process_batch(
    args: &Args,
    grok_path: &Path,
    tess_path: &Path,
    batch_dir: &Path,
    index_start: u32,
    output_dir: &Path,
    do_master: bool,
    do_user: bool,
    do_txt: bool,
    do_alto: bool,
    alto_version: &str,
    tessdata_dir: Option<&Path>,
    logs: &mut Vec<String>,
) -> Result<()> {
    let tiffs = collect_tiffs_in_dir(batch_dir)?;
    if tiffs.is_empty() {
        logs.push("Dávka neobsahuje žádné TIFF soubory – přeskočeno.".to_string());
        return Ok(());
    }

    if !(do_master || do_user || do_txt || do_alto) {
        logs.push("Nic není zapnuto (AC/UC/TXT/ALTO) – dávka přeskočena.".to_string());
        return Ok(());
    }

    let mut idx = index_start;
    let mut first_error: Option<anyhow::Error> = None;

    for tif in tiffs {
        let digits = args.digits;
        let index_str = format!("{:0digits$}", idx);
        logs.push(format!(
            "--- Soubor {} (index {}) ---",
            tif.display(),
            index_str
        ));

        if do_master {
            let out_jp2 = output_dir.join(format!("{index_str}.ac.jp2"));
            if let Err(e) = run_grok_master(grok_path, &tif, &out_jp2, args.dry_run, logs)
                .with_context(|| format!("Master JP2 selhalo pro `{}`", tif.display()))
            {
                logs.push(format!("Chyba Master: {e}"));
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }

        if do_user {
            let out_jp2 = output_dir.join(format!("{index_str}.uc.jp2"));
            if let Err(e) = run_grok_user(grok_path, &tif, &out_jp2, args.dry_run, logs)
                .with_context(|| format!("User JP2 selhalo pro `{}`", tif.display()))
            {
                logs.push(format!("Chyba User: {e}"));
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }

        // OCR - jednotné zpracování pro TXT i ALTO
        if do_txt || do_alto {
            if let Err(e) = run_tess_unified(
                tess_path,
                &args.lang,
                alto_version,
                args.dry_run,
                &tif,
                output_dir,
                &index_str,
                do_txt,
                do_alto,
                tessdata_dir,
                logs,
            )
            .with_context(|| format!("Tesseract selhalo pro `{}`", tif.display()))
            {
                logs.push(format!("Chyba OCR: {e}"));
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }

        idx += 1;
    }

    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(())
    }
}

fn command_to_string(label: &str, cmd: &Command) -> String {
    let program = cmd.get_program().to_string_lossy();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    format!("[{label}] {program} {}", args.join(" "))
}

/// Najde Grok binárku podle pravidel:
/// 1. Pokud grok_bin != "auto", použije se přímo
/// 2. Jinak zkusí lokální složky: ./grok/bin/grk_compress(.exe) nebo ./grok/grk_compress(.exe)
/// 3. Jinak grk_compress(.exe) z PATH
fn resolve_grok_path(grok_bin: &str) -> PathBuf {
    if grok_bin != "auto" {
        return PathBuf::from(grok_bin);
    }

    // 1. Lokální složka grok v kořenovém adresáři programu
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let grok_dir = exe_dir.join("grok");
            let grok_bin_dir = grok_dir.join("bin");

            if cfg!(windows) {
                // Zkusíme ./grok/bin/grk_compress.exe
                let candidate1 = grok_bin_dir.join("grk_compress.exe");
                if candidate1.exists() {
                    return candidate1;
                }
                // Zkusíme ./grok/grk_compress.exe
                let candidate2 = grok_dir.join("grk_compress.exe");
                if candidate2.exists() {
                    return candidate2;
                }
                // Linux/Unix varianta v bin složce
                let candidate3 = grok_bin_dir.join("grk_compress");
                if candidate3.exists() {
                    return candidate3;
                }
                // Linux/Unix varianta přímo v grok složce
                let candidate4 = grok_dir.join("grk_compress");
                if candidate4.exists() {
                    return candidate4;
                }
            } else {
                // Linux/Unix: ./grok/bin/grk_compress
                let candidate1 = grok_bin_dir.join("grk_compress");
                if candidate1.exists() {
                    return candidate1;
                }
                // Linux/Unix: ./grok/grk_compress
                let candidate2 = grok_dir.join("grk_compress");
                if candidate2.exists() {
                    return candidate2;
                }
            }
        }
    }

    // 2. PATH
    if cfg!(windows) {
        PathBuf::from("grk_compress.exe")
    } else {
        PathBuf::from("grk_compress")
    }
}

/// Najde Tesseract binárku s prioritou lokální složky programu
/// 1. Nejprve lokální složky: ./tesseract/bin/tesseract(.exe) nebo ./tesseract/tesseract(.exe)
/// 2. Pokud force_local_tess=true, pouze lokální složky, nikdy PATH
/// 3. Jinak zkusí standardní umístění (Windows) nebo PATH
fn resolve_tess_path_with_priority(tess_bin: &str, force_local_tess: bool) -> (PathBuf, String) {
    if tess_bin != "auto" {
        return (PathBuf::from(tess_bin), "explicitní".to_string());
    }

    // 1. Lokální složka tesseract v kořenovém adresáři programu - PRVNÍ PRIORITA
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let tess_dir = exe_dir.join("tesseract");
            let tess_bin_dir = tess_dir.join("bin");

            if cfg!(windows) {
                // Zkusíme ./tesseract/bin/tesseract.exe
                let candidate1 = tess_bin_dir.join("tesseract.exe");
                if candidate1.exists() {
                    return (candidate1, "lokální-složka/bin".to_string());
                }
                // Zkusíme ./tesseract/tesseract.exe
                let candidate2 = tess_dir.join("tesseract.exe");
                if candidate2.exists() {
                    return (candidate2, "lokální-složka".to_string());
                }
            } else {
                // Linux/Unix: ./tesseract/bin/tesseract
                let candidate1 = tess_bin_dir.join("tesseract");
                if candidate1.exists() {
                    return (candidate1, "lokální-složka/bin".to_string());
                }
                // Linux/Unix: ./tesseract/tesseract
                let candidate2 = tess_dir.join("tesseract");
                if candidate2.exists() {
                    return (candidate2, "lokální-složka".to_string());
                }
            }
        }
    }

    // 2. Pokud force_local_tess=true, nenajdeme nic lokálního -> chyba
    if force_local_tess {
        if cfg!(windows) {
            return (PathBuf::from("tesseract.exe"), "lokální-nenalezen".to_string());
        } else {
            return (PathBuf::from("tesseract"), "lokální-nenalezen".to_string());
        }
    }

    // 3. Windows standardní instalace
    if cfg!(windows) {
        let candidate = PathBuf::from(r"C:\Program Files\Tesseract-OCR\tesseract.exe");
        if candidate.exists() {
            return (candidate, "windows-instalace".to_string());
        }
        let candidate_alt =
            PathBuf::from(r"C:\Program Files (x86)\Tesseract-OCR\tesseract.exe");
        if candidate_alt.exists() {
            return (candidate_alt, "windows-instalace-x86".to_string());
        }
    }

    // 4. Hledání v PATH pomocí env::var
    if let Some(path_tess) = find_tesseract_in_path() {
        return (path_tess, "path".to_string());
    }

    // 5. Fallback
    if cfg!(windows) {
        (PathBuf::from("tesseract.exe"), "path-fallback".to_string())
    } else {
        (PathBuf::from("tesseract"), "path-fallback".to_string())
    }
}

/// Najde nadřazený adresář obsahující tessdata složku
fn find_tessdata_parent_dir(tesseract_path: &Path) -> Option<PathBuf> {
    // Prioritně hledáme vedle tesseract.exe
    if let Some(parent) = tesseract_path.parent() {
        // 1. Složka vedle tesseract.exe
        let tessdata_dir = parent.join("tessdata");
        if tessdata_dir.exists() && tessdata_dir.is_dir() {
            return Some(parent.to_path_buf());
        }
        
        // 2. Nadřazená složka (pro ./tesseract/bin/tesseract.exe -> ./tesseract/tessdata)
        if let Some(grandparent) = parent.parent() {
            let tessdata_dir = grandparent.join("tessdata");
            if tessdata_dir.exists() && tessdata_dir.is_dir() {
                return Some(grandparent.to_path_buf());
            }
            
            // 3. Pod složkou share (standardní Tesseract instalace)
            let share_tessdata = grandparent.join("share").join("tessdata");
            if share_tessdata.exists() && share_tessdata.is_dir() {
                return Some(grandparent.join("share"));
            }
        }
    }
    
    // 4. Aktuální pracovní adresář
    if let Ok(current_dir) = std::env::current_dir() {
        let tessdata_dir = current_dir.join("tessdata");
        if tessdata_dir.exists() && tessdata_dir.is_dir() {
            return Some(current_dir);
        }
    }
    
    // 5. Zkusíme TESSDATA_PREFIX z environmentu
    if let Ok(tessdata_prefix) = std::env::var("TESSDATA_PREFIX") {
        let tessdata_path = PathBuf::from(&tessdata_prefix).join("tessdata");
        if tessdata_path.exists() && tessdata_path.is_dir() {
            return Some(PathBuf::from(tessdata_prefix));
        }
    }
    
    None
}

fn check_grok(path: &Path, dry_run: bool) -> (ToolStatus, String) {
    if dry_run {
        return (ToolStatus::Ok("(dry-run)".to_string()), "?".to_string());
    }

    let mut cmd = Command::new(path);
    cmd.arg("-h"); // usage, ale exit code 0 pokud binárka funguje
    match cmd.output() {
        Ok(output) => {
            if output.status.success() {
                (ToolStatus::Ok("OK".to_string()), "?".to_string())
            } else {
                let code = output.status.code().unwrap_or(-1);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let first = stderr.lines().next().unwrap_or("");
                (
                    ToolStatus::Error(format!("exit {code}: {first}")),
                    "?".to_string(),
                )
            }
        }
        Err(e) => (ToolStatus::Error(format!("nelze spustit: {e}")), "?".to_string()),
    }
}

fn check_tesseract(path: &Path, dry_run: bool) -> ToolStatus {
    if dry_run {
        return ToolStatus::Ok("(dry-run)".to_string());
    }

    let mut cmd = Command::new(path);
    cmd.arg("--version");
    match cmd.output() {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let first = stdout.lines().next().unwrap_or("");
                ToolStatus::Ok(first.to_string())
            } else {
                let code = output.status.code().unwrap_or(-1);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let first = stderr.lines().next().unwrap_or("");
                ToolStatus::Error(format!("exit {code}: {first}"))
            }
        }
        Err(e) => ToolStatus::Error(format!("nelze spustit: {e}")),
    }
}

fn run_grok_master(
    grok_path: &Path,
    input: &Path,
    output_jp2: &Path,
    dry_run: bool,
    logs: &mut Vec<String>,
) -> Result<()> {
    let mut cmd = Command::new(grok_path);
    cmd.arg("-i")
        .arg(input)
        .arg("-o")
        .arg(output_jp2)
        .arg("-t")
        .arg("4096,4096")
        .arg("-p")
        .arg("RPCL")
        .arg("-n")
        .arg("6")
        .arg("-c")
        .arg("[256,256],[256,256],[128,128],[128,128],[128,128],[128,128]")
        .arg("-b")
        .arg("64,64")
        .arg("-X")
        .arg("-M")
        .arg("1")
        .arg("-S")
        .arg("-E")
        .arg("-u")
        .arg("R");

    let cmd_str = command_to_string("Grok Master", &cmd);
    logs.push(cmd_str);

    if dry_run {
        logs.push("(dry-run)".to_string());
        return Ok(());
    }

    let output = cmd
        .output()
        .context("Nelze spustit grk_compress (Master)")?;
    if output.status.success() {
        logs.push("OK".to_string());
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        logs.push(format!("FAILED: {stderr}"));
        Err(anyhow!("grk_compress Master selhalo"))
    }
}

fn run_grok_user(
    grok_path: &Path,
    input: &Path,
    output_jp2: &Path,
    dry_run: bool,
    logs: &mut Vec<String>,
) -> Result<()> {
    let mut cmd = Command::new(grok_path);
    cmd.arg("-i")
        .arg(input)
        .arg("-o")
        .arg(output_jp2)
        .arg("-r")
        .arg("362,256,181,128,90,64,45,32,22,16,11,8")
        .arg("-I")
        .arg("-t")
        .arg("1024,1024")
        .arg("-p")
        .arg("RPCL")
        .arg("-n")
        .arg("6")
        .arg("-c")
        .arg("[256,256],[256,256],[128,128],[128,128],[128,128],[128,128]")
        .arg("-b")
        .arg("64,64")
        .arg("-X")
        .arg("-M")
        .arg("1")
        .arg("-u")
        .arg("R")
        .arg("-H")
        .arg("4");

    let cmd_str = command_to_string("Grok User", &cmd);
    logs.push(cmd_str);

    if dry_run {
        logs.push("(dry-run)".to_string());
        return Ok(());
    }

    let output = cmd
        .output()
        .context("Nelze spustit grk_compress (User)")?;
    if output.status.success() {
        logs.push("OK".to_string());
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        logs.push(format!("FAILED: {stderr}"));
        Err(anyhow!("grk_compress User selhalo"))
    }
}

/// Zjednodušená verze - spustí Tesseract pro TXT i ALTO
fn run_tess_unified(
    tess_path: &Path,
    lang: &str,
    alto_version: &str,
    dry_run: bool,
    input: &Path,
    out_dir: &Path,
    index_str: &str,
    do_txt: bool,
    do_alto: bool,
    tessdata_dir: Option<&Path>,
    logs: &mut Vec<String>,
) -> Result<()> {
    // Základní informace
    logs.push(format!("Tesseract: {}", tess_path.display()));
    logs.push(format!("Jazyk: {}", lang));
    logs.push(format!("ALTO verze: {}", alto_version));

    // Základní název souboru (bez přípony)
    let out_base = out_dir.join(format!("{index_str}.ocr"));
    let mut cmd = Command::new(tess_path);
    
    // 1. Nastavíme TESSDATA_PREFIX a --tessdata-dir pokud máme tessdata_dir
    if let Some(tessdata_dir_path) = tessdata_dir {
        // Nejprve zkusíme, zda existuje tessdata složka uvnitř
        let tessdata_subdir = tessdata_dir_path.join("tessdata");
        if tessdata_subdir.exists() && tessdata_subdir.is_dir() {
            cmd.env("TESSDATA_PREFIX", tessdata_dir_path);
            cmd.arg("--tessdata-dir").arg(&tessdata_subdir);
            logs.push(format!("TESSDATA_PREFIX nastaven na: {}", tessdata_dir_path.display()));
            logs.push(format!("--tessdata-dir nastaven na: {}", tessdata_subdir.display()));
        } else if tessdata_dir_path.exists() {
            // Pokud přímo tessdata_dir_path existuje, použijeme ho jako --tessdata-dir
            cmd.env("TESSDATA_PREFIX", tessdata_dir_path.parent().unwrap_or(tessdata_dir_path));
            cmd.arg("--tessdata-dir").arg(tessdata_dir_path);
            logs.push(format!("TESSDATA_PREFIX nastaven na: {:?}", tessdata_dir_path.parent()));
            logs.push(format!("--tessdata-dir nastaven na: {}", tessdata_dir_path.display()));
        }
    } else {
        // Auto-detekce tessdata
        if let Some(tessdata_parent) = find_tessdata_parent_dir(tess_path) {
            let tessdata_dir = tessdata_parent.join("tessdata");
            if tessdata_dir.exists() && tessdata_dir.is_dir() {
                cmd.env("TESSDATA_PREFIX", &tessdata_parent);
                cmd.arg("--tessdata-dir").arg(&tessdata_dir);
                logs.push(format!("TESSDATA_PREFIX (auto): {}", tessdata_parent.display()));
                logs.push(format!("--tessdata-dir (auto): {}", tessdata_dir.display()));
            }
        } else {
            logs.push("Varování: Tessdata adresář nebyl nalezen".to_string());
        }
    }
    
    // 2. Mapujeme ALTO verzi
    let v = if alto_version.starts_with('4') { "4" } 
            else if alto_version.starts_with('3') { "3" } 
            else if alto_version.starts_with('2') { "2" } 
            else { "4" };

    // 3. Sestavíme příkaz
    cmd.arg(input)
        .arg(&out_base)
        .arg("-l").arg(lang);
    
    // Přidáme výstupní formáty
    let mut output_formats = Vec::new();
    if do_alto {
        output_formats.push("alto");
    }
    if do_txt {
        output_formats.push("txt");
    }
    
    if output_formats.is_empty() {
        logs.push("Žádné výstupní formáty nenastaveny - přeskočeno".to_string());
        return Ok(()); // Nic k vytvoření
    }
    
    for format in output_formats {
        cmd.arg(format);
    }
    
    // Přidáme konfiguraci pro ALTO
    if do_alto {
        cmd.arg("-c").arg("tessedit_create_alto=1")
            .arg("-c").arg(format!("alto_version={v}"));
    }

    let cmd_str = command_to_string("Tesseract", &cmd);
    logs.push(cmd_str);

    if dry_run {
        logs.push("(dry-run)".to_string());
        return Ok(());
    }

    logs.push("Spouštím Tesseract...".to_string());
    let output = cmd.output().context("Nelze spustit tesseract")?;
    
    if output.status.success() {
        logs.push("✓ Tesseract OK".to_string());
        
        // Kontrola vytvořených souborů
        if do_alto {
            let alto_file = out_dir.join(format!("{index_str}.ocr.xml"));
            if alto_file.exists() {
                logs.push(format!("✓ ALTO vytvořen: {}", alto_file.display()));
            } else {
                logs.push("✗ ALTO soubor nebyl vytvořen".to_string());
            }
        }
        
        if do_txt {
            let txt_file = out_dir.join(format!("{index_str}.ocr.txt"));
            if txt_file.exists() {
                logs.push(format!("✓ TXT vytvořen: {}", txt_file.display()));
            } else {
                logs.push("✗ TXT soubor nebyl vytvořen".to_string());
            }
        }
        
        Ok(())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let status_code = output.status.code().unwrap_or(-1);
        
        logs.push(format!("✗ Tesseract FAILED (exit code: {})", status_code));
        if !stdout.is_empty() {
            logs.push(format!("STDOUT: {}", stdout));
        }
        if !stderr.is_empty() {
            logs.push(format!("STDERR: {}", stderr));
        }
        
        Err(anyhow!("Tesseract selhalo s exit code {}", status_code))
    }
}

/// Zapišeme manifest a log.txt s informacemi o běhu
fn write_manifest_and_log_with_process_logs(
    manifest: &manifest::BatchManifest,
    logs_dir: &Path,
    process_logs: &[String],
) -> Result<()> {
    use std::fs;
    
    // Vytvoříme log_dir pokud neexistuje
    fs::create_dir_all(logs_dir)?;
    
    // Manifest
    let manifest_path = logs_dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    fs::write(&manifest_path, manifest_json)?;
    
    // Log.txt
    let log_path = logs_dir.join("log.txt");
    let mut log_content = String::new();
    
    // Hlavička
    log_content.push_str("BATCH PROCESSING LOG\n");
    log_content.push_str("===================\n\n");
    log_content.push_str(&format!("Batch: {}\n", manifest.batch_name));
    log_content.push_str(&format!("Start index: {}\n", manifest.start_index));
    log_content.push_str(&format!("File count: {}\n", manifest.file_count));
    log_content.push_str(&format!("Generated: {}\n", manifest.generated));
    log_content.push_str(&format!("Language: {}\n", manifest.lang));
    log_content.push_str(&format!("ALTO version: {}\n", manifest.alto_version));
    log_content.push_str(&format!("Formats: AC={}, UC={}, TXT={}, ALTO={}\n", 
        manifest.pages.iter().any(|p| p.ac_jp2.is_some()),
        manifest.pages.iter().any(|p| p.uc_jp2.is_some()),
        manifest.pages.iter().any(|p| p.txt.is_some()),
        manifest.pages.iter().any(|p| p.alto.is_some())));
    
    log_content.push_str("\n");
    log_content.push_str(&"=".repeat(80));
    log_content.push_str("\nPROCESS EXECUTION LOG\n");
    log_content.push_str(&"=".repeat(80));
    log_content.push_str("\n\n");
    
    // Přidáme všechny logy z procesu
    for log_line in process_logs {
        log_content.push_str(log_line);
        log_content.push_str("\n");
    }
    
    log_content.push_str("\n");
    log_content.push_str(&"=".repeat(80));
    log_content.push_str("\nFILE CHECKSUMS\n");
    log_content.push_str(&"=".repeat(80));
    log_content.push_str("\n\n");
    
    // Přidáme checksumy
    for page in &manifest.pages {
        log_content.push_str(&format!("[Page {}]\n", page.index));
        log_content.push_str(&format!("  Original TIFF: {}\n", 
            page.original_tiff.path));
        
        if let Some(ref ac_jp2) = page.ac_jp2 {
            log_content.push_str(&format!("  AC JP2: {} = {}\n",
                ac_jp2.path, ac_jp2.blake3));
        }
        
        if let Some(ref uc_jp2) = page.uc_jp2 {
            log_content.push_str(&format!("  UC JP2: {} = {}\n",
                uc_jp2.path, uc_jp2.blake3));
        }
        
        if let Some(ref txt) = page.txt {
            log_content.push_str(&format!("  TXT: {} = {}\n",
                txt.path, txt.blake3));
        }
        
        if let Some(ref alto) = page.alto {
            log_content.push_str(&format!("  ALTO: {} = {}\n",
                alto.path, alto.blake3));
        }
        log_content.push_str("\n");
    }
    
    fs::write(&log_path, log_content)?;
    
    Ok(())
}