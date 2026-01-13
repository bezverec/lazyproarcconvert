use anyhow::{Context, Result};
use clap::Parser;
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    thread,
};

/// Malý statický HTTP server pro prohlížení HTML reportů (index.html)
/// a souvisejících souborů (ALTO, TXT, WebP, JP2).
#[derive(Parser, Debug)]
struct Args {
    /// Port, na kterém bude server poslouchat
    #[arg(long, default_value_t = 8000)]
    port: u16,

    /// Kořenový adresář pro statické soubory (volitelné).
    /// Pokud není zadán, vezme se automaticky `<adresář_exe>/output`,
    /// a pokud neexistuje, použije se adresář s binárkou.
    #[arg(long)]
    root: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let root_dir = resolve_root_dir(args.root.as_deref())?;
    println!(
        "LazyALTO server: sloužím soubory z: {}",
        root_dir.display()
    );

    let url = format!("http://localhost:{}/", args.port);
    println!("Otevři v prohlížeči: {url}");
    println!("(v rootu uvidíš seznam *_logs adresářů)");

    // Zkusíme automaticky otevřít prohlížeč (na pozadí, chyby ignorujeme)
    open_in_browser(&url);

    // Běžíme jen na localhostu
    let listener = TcpListener::bind(("127.0.0.1", args.port))
        .with_context(|| format!("Nelze bindnout port {}", args.port))?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let root = root_dir.clone();
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, &root) {
                        eprintln!("Chyba při obsluze spojení: {e}");
                    }
                });
            }
            Err(e) => eprintln!("Chyba při accept(): {e}"),
        }
    }

    Ok(())
}

/// Určí kořenový adresář serveru.
///
/// Priorita:
/// 1) `--root <cesta>`
/// 2) `<adresář_exe>/output` pokud existuje
/// 3) `<adresář_exe>`
fn resolve_root_dir(root_arg: Option<&str>) -> Result<PathBuf> {
    if let Some(r) = root_arg {
        let p = PathBuf::from(r);
        if !p.is_dir() {
            anyhow::bail!("Zadaný root `{}` není adresář nebo neexistuje", p.display());
        }
        return Ok(p);
    }

    let exe_dir = std::env::current_exe()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(std::env::current_dir()?);

    let candidate = exe_dir.join("output");
    if candidate.is_dir() {
        Ok(candidate)
    } else {
        Ok(exe_dir)
    }
}

fn handle_client(mut stream: TcpStream, root_dir: &Path) -> Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buf[..n]);
    let mut lines = request.lines();

    let first_line = match lines.next() {
        Some(l) => l,
        None => return Ok(()),
    };

    // Očekáváme něco jako: GET /cesta HTTP/1.1
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    if method != "GET" {
        write_response(
            &mut stream,
            405,
            "Method Not Allowed",
            "text/plain; charset=utf-8",
            b"Only GET is supported",
        )?;
        return Ok(());
    }

    // Jednoduchá ochrana proti .. v cestě
    if path.contains("..") {
        write_response(
            &mut stream,
            400,
            "Bad Request",
            "text/plain; charset=utf-8",
            b"Invalid path",
        )?;
        return Ok(());
    }

    // Favicon jen ignorujeme s prázdnou odpovědí
    if path == "/favicon.ico" {
        write_response(
            &mut stream,
            404,
            "Not Found",
            "text/plain; charset=utf-8",
            b"Not found",
        )?;
        return Ok(());
    }

    // Pokud se jde na kořen "/", ukážeme stylizovaný index s *_logs adresáři
    let rel_path = if path == "/" {
        return serve_root_index(&mut stream, root_dir);
    } else {
        &path[1..] // odřízneme počáteční '/'
    };

    // Převedeme URL na cestu v FS (forward slashes -> platform separator)
    let rel_for_fs = rel_path.replace('/', &std::path::MAIN_SEPARATOR.to_string());
    let fs_path = root_dir.join(rel_for_fs);

    if fs_path.is_dir() {
        // Pokud je to adresář, zkusíme tamní index.html
        let index_html = fs_path.join("index.html");
        if index_html.is_file() {
            return serve_file(&mut stream, &index_html);
        } else {
            // jinak vypíšeme jednoduchý listing (taky lehce nastylovaný)
            return serve_dir_listing(&mut stream, &fs_path, path);
        }
    }

    if fs_path.is_file() {
        return serve_file(&mut stream, &fs_path);
    }

    write_response(
        &mut stream,
        404,
        "Not Found",
        "text/plain; charset=utf-8",
        b"File not found",
    )?;

    Ok(())
}

/// Stylizovaná homepage na / s výčtem *_logs adresářů
fn serve_root_index(stream: &mut TcpStream, root_dir: &Path) -> Result<()> {
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
      min-height: 100vh;
      display: flex;
      flex-direction: column;
    }

    header {
      padding: 14px 18px;
      border-bottom: 1px solid var(--border-soft);
      background: linear-gradient(90deg, #12141f, #191c2b);
      display: flex;
      flex-wrap: wrap;
      align-items: baseline;
      gap: 10px;
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
      padding: 16px;
      display: flex;
      flex-direction: column;
      gap: 16px;
    }

    .panel {
      background: var(--bg-panel);
      border-radius: 12px;
      border: 1px solid var(--border-soft);
      padding: 12px 14px;
      max-width: 980px;
      margin: 0 auto;
      box-shadow: 0 12px 40px rgba(0,0,0,0.55);
    }

    .panel h2 {
      margin: 0 0 8px 0;
      font-size: 14px;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      color: var(--text-muted);
      display: flex;
      justify-content: space-between;
      align-items: center;
    }

    .intro {
      font-size: 13px;
      color: var(--text-muted);
      line-height: 1.5;
      margin-bottom: 10px;
    }

    .logs-list {
      list-style: none;
      padding: 0;
      margin: 0;
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
      gap: 8px;
    }

    .logs-item a {
      display: block;
      padding: 10px 12px;
      border-radius: 8px;
      background: radial-gradient(circle at top left, rgba(79,195,247,0.12) 0, #101222 55%);
      border: 1px solid rgba(79,195,247,0.3);
      color: var(--text-main);
      text-decoration: none;
      font-size: 13px;
      transition: all 0.15s ease-out;
    }

    .logs-item a:hover {
      background: rgba(79,195,247,0.2);
      transform: translateY(-1px);
      box-shadow: 0 4px 14px rgba(79,195,247,0.25);
    }

    .logs-item-title {
      font-weight: 600;
      margin-bottom: 2px;
    }

    .logs-item-sub {
      font-size: 11px;
      color: var(--text-muted);
    }

    .hint {
      font-size: 12px;
      color: var(--text-muted);
      margin-top: 8px;
    }

    .badge {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      padding: 2px 8px;
      border-radius: 999px;
      background: var(--accent-soft);
      border: 1px solid rgba(79,195,247,0.4);
      font-size: 11px;
      color: var(--accent);
    }

    .no-logs {
      font-size: 13px;
      color: var(--text-muted);
      padding: 12px;
      border-radius: 8px;
      background: #101222;
      border: 1px dashed var(--border-soft);
    }

    footer {
      font-size: 11px;
      color: var(--text-muted);
      text-align: center;
      padding: 8px 0 12px;
    }

    code {
      font-family: Consolas, Menlo, Monaco, monospace;
      font-size: 12px;
    }
  </style>
</head>
<body>
<header>
  <h1>LazyProArcConvert – ALTO náhled &amp; editor</h1>
  <div class="meta">
    Výstupní logy & náhledy · <span class="badge">HTTP server běží na localhostu</span>
  </div>
</header>
<main>
  <section class="panel">
    <h2>Dostupné dávky</h2>
    <p class="intro">
      Vyber dávku (<code>*_logs</code>) pro otevření interaktivního ALTO náhledu
      a editoru. Stránka s náhledem je generovaná pro každý batch jako <code>index.html</code>.
    </p>
"#,
    );

    // seznam *_logs
    let mut any_logs = false;

    html.push_str(r#"<ul class="logs-list">"#);
    if let Ok(entries) = fs::read_dir(root_dir) {
        let mut names: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.ends_with("_logs") {
                        names.push(name);
                    }
                }
            }
        }
        names.sort();

        for name in names {
            any_logs = true;
            // odřízneme suffix _logs pro "batch" jméno
            let batch_label = name.trim_end_matches("_logs");
            html.push_str(r#"<li class="logs-item"><a href="/"#);
            html.push_str(&html_escape(&name));
            html.push_str(r#"/index.html">"#);
            html.push_str(r#"<div class="logs-item-title">"#);
            html.push_str(&html_escape(batch_label));
            html.push_str(r#"</div>"#);
            html.push_str(r#"<div class="logs-item-sub">"#);
            html.push_str(&html_escape(&name));
            html.push_str(r#"</div></a></li>"#);
        }
    }
    html.push_str("</ul>");

    if !any_logs {
        html.push_str(
            r#"<div class="no-logs">
        Nenalezeny žádné <code>*_logs</code> adresáře.<br>
        Ujisti se, že jsi spustil <code>lazyproarcconvert</code> a máš v okolí
        buď adresář <code>output</code> s logy, nebo spouštíš server přímo v adresáři s logy.
      </div>"#,
        );
    }

    html.push_str(
        r#"<p class="hint">
      Tip: server můžeš spustit jednoduše jako samostatný EXE (<code>lazyalto.exe</code>).
      Na Windows se prohlížeč otevře automaticky.
    </p>
  </section>
</main>
<footer>
  LazyProArcConvert &middot; ALTO náhled &amp; editor · statický server <code>lazyalto</code>
</footer>
</body>
</html>"#,
    );

    write_response(
        stream,
        200,
        "OK",
        "text/html; charset=utf-8",
        html.as_bytes(),
    )
}

fn serve_dir_listing(stream: &mut TcpStream, dir: &Path, url_path: &str) -> Result<()> {
    let mut html = String::new();
    html.push_str(
        r#"<!DOCTYPE html>
<html lang="cs">
<head>
  <meta charset="utf-8">
  <title>Index</title>
  <style>
    body {
      margin: 0;
      padding: 12px 16px;
      background: radial-gradient(circle at top, #202542 0, #050611 55%);
      color: #f5f7ff;
      font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }
    h1 { font-size: 18px; margin: 0 0 10px 0; }
    ul { list-style: none; padding: 0; margin: 0; }
    li { margin-bottom: 4px; }
    a {
      color: #4fc3f7;
      text-decoration: none;
      padding: 4px 8px;
      border-radius: 6px;
      display: inline-block;
      background: rgba(22,24,36,0.9);
      border: 1px solid #2a2f40;
      font-size: 13px;
    }
    a:hover {
      background: rgba(79,195,247,0.16);
      border-color: #4fc3f7;
    }
  </style>
</head>
<body>"#,
    );

    html.push_str("<h1>Index ");
    html.push_str(&html_escape(url_path));
    html.push_str("</h1><ul>");

    if let Ok(entries) = fs::read_dir(dir) {
        let mut names: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            names.push(entry.file_name().to_string_lossy().to_string());
        }
        names.sort();

        for name in names {
            html.push_str("<li><a href=\"");
            if url_path.ends_with('/') {
                html.push_str(url_path);
                html.push_str(&name);
            } else {
                html.push_str(url_path);
                html.push('/');
                html.push_str(&name);
            }
            html.push_str("\">");
            html.push_str(&html_escape(&name));
            html.push_str("</a></li>");
        }
    }

    html.push_str("</ul></body></html>");

    write_response(
        stream,
        200,
        "OK",
        "text/html; charset=utf-8",
        html.as_bytes(),
    )
}

fn serve_file(stream: &mut TcpStream, path: &Path) -> Result<()> {
    let data = fs::read(path)?;
    let mime = guess_mime(path);

    write_response(stream, 200, "OK", &mime, &data)
}

fn write_response(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    let mut headers = Vec::new();
    headers.extend_from_slice(format!("HTTP/1.1 {} {}\r\n", status_code, status_text).as_bytes());
    headers.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    headers.extend_from_slice(format!("Content-Type: {}\r\n", content_type).as_bytes());
    headers.extend_from_slice(b"Connection: close\r\n");
    headers.extend_from_slice(b"\r\n");

    stream.write_all(&headers)?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

fn guess_mime(path: &Path) -> String {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8".to_string(),
        "css" => "text/css; charset=utf-8".to_string(),
        "js" => "text/javascript; charset=utf-8".to_string(),
        "json" => "application/json; charset=utf-8".to_string(),
        "xml" => "application/xml; charset=utf-8".to_string(),
        "txt" => "text/plain; charset=utf-8".to_string(),
        "webp" => "image/webp".to_string(),
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "png" => "image/png".to_string(),
        "jp2" => "image/jp2".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

fn html_escape(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Otevře URL v defaultním prohlížeči (best-effort, chyby se ignorují).
fn open_in_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(url).spawn();
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}
