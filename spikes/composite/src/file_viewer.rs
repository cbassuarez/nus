//! Local document rendering. The browser serves this HTML at the ORIGINAL file
//! URL, so relative links and images resolve without copying a document tree.
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::Path,
    sync::{Arc, RwLock},
};
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Markdown,
    Json,
    Csv,
    Text,
}
impl Kind {
    pub const ALL: [Self; 4] = [Self::Markdown, Self::Json, Self::Csv, Self::Text];
    pub fn label(self) -> &'static str {
        match self {
            Self::Markdown => "Markdown",
            Self::Json => "JSON",
            Self::Csv => "CSV",
            Self::Text => "Plain text",
        }
    }
    pub fn of(path: &Path) -> Option<Self> {
        Some(
            match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
                "md" | "markdown" | "mdown" => Self::Markdown,
                "json" => Self::Json,
                "csv" | "tsv" => Self::Csv,
                "txt" | "log" => Self::Text,
                _ => return None,
            },
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Follow,
    Paper,
    Ink,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Font {
    Serif,
    Sans,
    Mono,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub enabled: bool,
    pub markdown: bool,
    pub json: bool,
    pub csv: bool,
    pub text: bool,
    pub theme: Theme,
    pub font: Font,
    pub text_size: u8,
    pub width: u16,
    pub line_height: u16,
    pub wrap: bool,
    pub local_images: bool,
    pub remote_images: bool,
    pub csv_header: bool,
    pub contents: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: true,
            markdown: true,
            json: true,
            csv: true,
            text: true,
            theme: Theme::Follow,
            font: Font::Serif,
            text_size: 18,
            width: 860,
            line_height: 170,
            wrap: true,
            local_images: true,
            remote_images: false,
            csv_header: true,
            contents: true,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Enabled(bool),
    Format(Kind, bool),
    Theme(Theme),
    Font(Font),
    Size(u8),
    Width(u16),
    LineHeight(u16),
    Wrap(bool),
    LocalImages(bool),
    RemoteImages(bool),
    CsvHeader(bool),
    Contents(bool),
}
impl Setting {
    pub fn label(self) -> String {
        match self {
            Self::Enabled(v) => if v {
                "Open supported files as formatted documents."
            } else {
                "Open files without nus document formatting."
            }
            .into(),
            Self::Format(k, v) => format!(
                "{} {} files.",
                if v {
                    "Show a formatted view of"
                } else {
                    "Use the source for"
                },
                k.label()
            ),
            Self::Theme(Theme::Follow) => "Follow the app’s current colors.".into(),
            Self::Theme(Theme::Paper) => "Always use a light page.".into(),
            Self::Theme(Theme::Ink) => "Always use a dark page.".into(),
            Self::Font(Font::Serif) => "Use serif type for document text.".into(),
            Self::Font(Font::Sans) => "Use sans serif type for document text.".into(),
            Self::Font(Font::Mono) => "Use monospace type for document text.".into(),
            Self::Size(v) => format!("Set document text to {v} pixels."),
            Self::Width(v) => format!("Limit the page to {v} pixels wide."),
            Self::LineHeight(v) => format!("Set line spacing to {v}%."),
            Self::Wrap(v) => if v {
                "Wrap long text and code lines."
            } else {
                "Scroll long text and code horizontally."
            }
            .into(),
            Self::LocalImages(v) => if v {
                "Show images linked from local files."
            } else {
                "Hide local images."
            }
            .into(),
            Self::RemoteImages(v) => if v {
                "Load images from linked web servers."
            } else {
                "Do not request remote images."
            }
            .into(),
            Self::CsvHeader(v) => if v {
                "Treat the first row as column headings."
            } else {
                "Treat every row as data."
            }
            .into(),
            Self::Contents(v) => if v {
                "Show a collapsible table of contents."
            } else {
                "Hide the table of contents."
            }
            .into(),
        }
    }
}
impl Preferences {
    pub fn allows(&self, kind: Kind) -> bool {
        self.enabled
            && match kind {
                Kind::Markdown => self.markdown,
                Kind::Json => self.json,
                Kind::Csv => self.csv,
                Kind::Text => self.text,
            }
    }
    pub fn set(&mut self, s: Setting) {
        match s {
            Setting::Enabled(v) => self.enabled = v,
            Setting::Format(k, v) => {
                *match k {
                    Kind::Markdown => &mut self.markdown,
                    Kind::Json => &mut self.json,
                    Kind::Csv => &mut self.csv,
                    Kind::Text => &mut self.text,
                } = v
            }
            Setting::Theme(v) => self.theme = v,
            Setting::Font(v) => self.font = v,
            Setting::Size(v) => self.text_size = v.clamp(12, 28),
            Setting::Width(v) => self.width = v.clamp(520, 1600),
            Setting::LineHeight(v) => self.line_height = v.clamp(120, 220),
            Setting::Wrap(v) => self.wrap = v,
            Setting::LocalImages(v) => self.local_images = v,
            Setting::RemoteImages(v) => self.remote_images = v,
            Setting::CsvHeader(v) => self.csv_header = v,
            Setting::Contents(v) => self.contents = v,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub prefs: Preferences,
    pub paper: [f32; 4],
    pub ink: [f32; 4],
    pub accent: [f32; 4],
}
impl Default for Config {
    fn default() -> Self {
        Self {
            prefs: Preferences::default(),
            paper: [0.98, 0.97, 0.94, 1.0],
            ink: [0.12, 0.13, 0.14, 1.0],
            accent: [0.1, 0.4, 0.5, 1.0],
        }
    }
}
pub type Shared = Arc<RwLock<Config>>;
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn safe_url(s: &str, image: bool, p: &Preferences) -> bool {
    let base = url::Url::parse("file:///document/root.md").unwrap();
    let Ok(u) = base.join(s) else {
        return false;
    };
    if !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    match u.scheme() {
        "file" => u.host_str().is_none_or(|h| h == "localhost") && (!image || p.local_images),
        "http" | "https" => !image || p.remote_images,
        "mailto" => !image,
        _ => false,
    }
}
fn markdown(source: &str, p: &Preferences) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let mut heads = Vec::new();
    let mut heading = None;
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => heading = Some((level, String::new())),
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, text)) = &mut heading {
                    text.push_str(&t);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(h) = heading.take() {
                    heads.push(h);
                }
            }
            _ => {}
        }
    }
    let mut used = std::collections::HashMap::new();
    let ids: Vec<String> = heads
        .iter()
        .map(|(_, s)| {
            let base = s
                .to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
                .map(|c| if c.is_whitespace() { '-' } else { c })
                .collect::<String>();
            let base = if base.is_empty() {
                "section".into()
            } else {
                base
            };
            let count = used.entry(base.clone()).or_insert(0usize);
            let id = if *count == 0 {
                base
            } else {
                format!("{base}-{count}")
            };
            *count += 1;
            id
        })
        .collect();
    let mut index = 0;
    let events = Parser::new_ext(source, options).map(|event| match event {
        Event::Html(t) | Event::InlineHtml(t) => Event::Text(t),
        Event::Start(Tag::Heading {
            level,
            classes,
            attrs,
            ..
        }) => {
            let id = ids[index].clone();
            index += 1;
            Event::Start(Tag::Heading {
                level,
                id: Some(id.into()),
                classes,
                attrs,
            })
        }
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: if safe_url(&dest_url, false, p) {
                dest_url
            } else {
                "#".into()
            },
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: if safe_url(&dest_url, true, p) {
                dest_url
            } else {
                "".into()
            },
            title,
            id,
        }),
        other => other,
    });
    let mut out = String::new();
    if p.contents && !heads.is_empty() {
        out.push_str(
            "<details class='contents'><summary>Contents</summary><nav aria-label='Contents'><ul>",
        );
        for ((_, text), id) in heads.iter().zip(&ids) {
            out.push_str(&format!(
                "<li><a href='#{}'>{}</a></li>",
                escape(id),
                escape(text)
            ));
        }
        out.push_str("</ul></nav></details>");
    }
    pulldown_cmark::html::push_html(&mut out, events);
    out
}
fn json_tree(value: &serde_json::Value, depth: usize, nodes: &mut usize) -> String {
    *nodes += 1;
    if *nodes > 20000 {
        return "<span>Additional values omitted from the tree. Use Source for the complete file.</span>".into();
    }
    let pairs: Option<Vec<(String, &serde_json::Value)>> = match value {
        serde_json::Value::Object(o) => Some(o.iter().map(|(k, v)| (k.clone(), v)).collect()),
        serde_json::Value::Array(a) => Some(
            a.iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v))
                .collect(),
        ),
        _ => None,
    };
    if let Some(pairs) = pairs {
        let mut html = format!(
            "<details {}><summary>{} · {} {}</summary><dl>",
            if depth < 2 { "open" } else { "" },
            if value.is_array() { "Array" } else { "Object" },
            pairs.len(),
            if value.is_array() { "items" } else { "keys" }
        );
        for (key, value) in pairs {
            if *nodes > 20000 {
                html.push_str("<dt>Tree limit reached; see Source below.</dt>");
                break;
            }
            html.push_str(&format!(
                "<dt>{}</dt><dd>{}</dd>",
                escape(&key),
                json_tree(value, depth + 1, nodes)
            ));
        }
        html.push_str("</dl></details>");
        html
    } else {
        format!("<code class='value'>{}</code>", escape(&value.to_string()))
    }
}
fn csv_table(source: &str, tsv: bool, header: bool) -> Result<String, String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(if tsv { b'\t' } else { b',' })
        .from_reader(source.as_bytes());
    let mut html = String::from(
        "<div class='table-scroll' tabindex='0' role='region' aria-label='Data table'><table>",
    );
    let mut cells = 0;
    for (i, row) in reader.records().enumerate() {
        let row = row.map_err(|e| e.to_string())?;
        cells += row.len();
        if i >= 10000 || cells > 100000 {
            return Err("This table exceeds the 10,000 row / 100,000 cell viewer limit. Open the source in the editor.".into());
        }
        if i == 0 && header {
            html.push_str("<thead>");
        } else if i == usize::from(header) {
            html.push_str("<tbody>");
        }
        html.push_str("<tr>");
        for cell in row.iter() {
            let tag = if i == 0 && header {
                "th scope='col'"
            } else {
                "td"
            };
            let end = if i == 0 && header { "th" } else { "td" };
            html.push_str(&format!("<{tag}>{}</{end}>", escape(cell)));
        }
        html.push_str("</tr>");
        if i == 0 && header {
            html.push_str("</thead>");
        }
    }
    html.push_str("</tbody></table></div>");
    Ok(html)
}
pub fn render(path: &Path, source: &str, config: &Config) -> String {
    let p = &config.prefs;
    let kind = Kind::of(path).unwrap_or(Kind::Text);
    let body = match kind {
        Kind::Markdown => markdown(source, p),
        Kind::Json => match serde_json::from_str::<serde_json::Value>(source) {
            Ok(value) => format!(
                "{}<details><summary>Source</summary><pre>{}</pre></details>",
                json_tree(&value, 0, &mut 0),
                escape(&serde_json::to_string_pretty(&value).unwrap())
            ),
            Err(e) => format!(
                "<p role='alert'>Invalid JSON: {}</p><pre>{}</pre>",
                escape(&e.to_string()),
                escape(source)
            ),
        },
        Kind::Csv => csv_table(
            source,
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("tsv")),
            p.csv_header,
        )
        .unwrap_or_else(|e| format!("<p role='alert'>{}</p>", escape(&e))),
        Kind::Text => format!("<pre class='text'>{}</pre>", escape(source)),
    };
    document(path, kind, &body, config)
}
fn document(path: &Path, kind: Kind, body: &str, config: &Config) -> String {
    let p = &config.prefs;
    let rgb = |c: [f32; 4]| {
        format!(
            "#{:02x}{:02x}{:02x}",
            (c[0].clamp(0.0, 1.0) * 255.0) as u8,
            (c[1].clamp(0.0, 1.0) * 255.0) as u8,
            (c[2].clamp(0.0, 1.0) * 255.0) as u8
        )
    };
    let (bg, fg, accent) = match p.theme {
        Theme::Follow => (rgb(config.paper), rgb(config.ink), rgb(config.accent)),
        Theme::Paper => ("#faf8f2".into(), "#24272b".into(), "#176075".into()),
        Theme::Ink => ("#1d2025".into(), "#ecebe6".into(), "#85cddb".into()),
    };
    let images = format!(
        "{} {}",
        if p.local_images { "file:" } else { "" },
        if p.remote_images { "https: http:" } else { "" }
    );
    let images = if images.trim().is_empty() {
        "'none'"
    } else {
        images.trim()
    };
    let title = escape(&path.file_name().unwrap_or_default().to_string_lossy());
    let font = match p.font {
        Font::Serif => "Georgia,serif",
        Font::Sans => "system-ui,sans-serif",
        Font::Mono => "ui-monospace,monospace",
    };
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src {images}; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; object-src 'none'"><title>{title}</title><style>
:root{{color-scheme:light dark;--paper:{bg};--ink:{fg};--accent:{accent}}}*{{box-sizing:border-box}}html{{background:var(--paper);color:var(--ink)}}body{{margin:0;font:{size}px/{line} {font}}}main{{max-width:{width}px;margin:auto;padding:28px clamp(16px,4vw,48px) 80px;overflow-wrap:anywhere}}header{{border-bottom:1px solid color-mix(in srgb,var(--ink) 22%,transparent);padding-bottom:18px;margin-bottom:30px;font:12px/1.6 system-ui,sans-serif}}header h1{{font-size:16px;margin:4px 0}}header p{{margin:0;opacity:.7}}a{{color:var(--accent)}}a:focus-visible,summary:focus-visible{{outline:2px solid var(--accent);outline-offset:4px}}h1,h2,h3,h4{{line-height:1.2;scroll-margin-top:24px}}h1{{font-size:2.1em}}h2{{margin-top:1.7em}}img{{max-width:100%;height:auto}}pre,code{{font-family:ui-monospace,monospace;font-size:.85em}}pre{{white-space:{wrap};overflow:auto;padding:18px;background:color-mix(in srgb,var(--ink) 5%,var(--paper));border:1px solid color-mix(in srgb,var(--ink) 14%,transparent);border-radius:4px}}pre code{{font-size:1em}}blockquote{{border-left:3px solid var(--accent);margin:24px 0;padding:0 20px}}.table-scroll{{overflow:auto;max-height:75vh}}table{{border-collapse:collapse;width:100%;font-family:system-ui,sans-serif;font-size:.85em}}th,td{{border-bottom:1px solid color-mix(in srgb,var(--ink) 18%,transparent);text-align:left;vertical-align:top;padding:10px 14px;white-space:pre-wrap}}th{{position:sticky;top:0;background:var(--paper)}}tr:nth-child(even){{background:color-mix(in srgb,var(--ink) 4%,var(--paper))}}details{{margin:12px 0}}summary{{cursor:pointer;color:var(--accent);font-family:system-ui,sans-serif}}dl{{margin-left:18px;border-left:1px solid color-mix(in srgb,var(--ink) 20%,transparent);padding-left:16px}}dt{{font:bold .8em/1.5 ui-monospace,monospace}}dd{{margin:4px 0 14px 12px}}.contents{{font-size:.85em}}.value{{white-space:pre-wrap}}hr{{border:0;border-top:1px solid var(--accent)}}@media print{{header,.contents{{display:none}}main{{max-width:none}}details{{display:block}}}}
</style></head><body><main><header><span>{kind} · FILE VIEWER</span><h1>{title}</h1><p>Right-click to edit source · Customize in Settings → File viewers</p></header><article>{body}</article></main></body></html>"#,
        size = p.text_size.clamp(12, 28),
        line = p.line_height.clamp(120, 220) as f32 / 100.0,
        width = p.width.clamp(520, 1600),
        wrap = if p.wrap { "pre-wrap" } else { "pre" },
        kind = kind.label()
    )
}
/// Called on CEF's resource thread. Size limits are applied before allocation.
pub fn load(url: &str, config: &Config) -> Option<Vec<u8>> {
    let url = url::Url::parse(url).ok()?;
    let path = url.to_file_path().ok()?;
    let kind = Kind::of(&path)?;
    if !config.prefs.allows(kind) {
        return None;
    }
    let result = (|| -> Result<String, String> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // A named pipe with a .md/.txt suffix must not block the CEF I/O
            // thread waiting for a writer before we can check its file type.
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = options.open(&path).map_err(|e| e.to_string())?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.len() > MAX_BYTES {
            return Err(
                "File exceeds the 8 MiB viewer limit. Open the source in the editor.".into(),
            );
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("File grew beyond the viewer limit.".into());
        }
        String::from_utf8(bytes).map_err(|_| {
            "This file is not UTF-8 text. Open it in an external editor to choose its encoding."
                .into()
        })
    })();
    Some(
        match result {
            Ok(source) => render(&path, &source, config),
            Err(e) => document(
                &path,
                kind,
                &format!("<p role='alert'>{}</p>", escape(&e)),
                config,
            ),
        }
        .into_bytes(),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn named_pipes_are_rejected_without_waiting_for_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pipe.md");
        let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        let url = url::Url::from_file_path(&path).unwrap();
        let html = load(url.as_str(), &Config::default()).unwrap();
        assert!(String::from_utf8(html).unwrap().contains("role='alert'"));
    }
    #[test]
    fn markdown_preserves_relative_assets_and_anchors_without_active_html() {
        let h=render(Path::new("a.md"),"# Hello world\n[Jump](#hello-world)\n![Local](images/a%20b.png)\n[Next](../next.md)\n<script>alert(1)</script>\n[bad](javascript:alert)\n![Remote](https://example.test/a.png)",&Config::default());
        assert!(h.contains("id=\"hello-world\""));
        assert!(h.contains("src=\"images/a%20b.png\""));
        assert!(h.contains("href=\"../next.md\""));
        assert!(!h.contains("<script>"));
        assert!(!h.contains("javascript:"));
        assert!(!h.contains("src=\"https:"));
    }
    #[test]
    fn quoted_csv_and_json_are_safe() {
        let h = csv_table("name,note\nAda,\"line one\nline two, yes\"\n", false, true).unwrap();
        assert!(h.contains("line one\nline two, yes"));
        assert!(h.contains("scope='col'"));
        let h = render(
            Path::new("x.json"),
            "{\"x\":\"<img src=x onerror=evil()>\"}",
            &Config::default(),
        );
        assert!(h.contains("&lt;img"));
        assert!(!h.contains("<img src=x"));
        assert!(h.contains("<details open>"));
    }
    #[test]
    fn toggles_and_theme_roundtrip() {
        let mut c = Config::default();
        c.prefs.set(Setting::Format(Kind::Markdown, false));
        assert!(!c.prefs.allows(Kind::Markdown));
        assert!(c.prefs.allows(Kind::Json));
        c.prefs.set(Setting::Theme(Theme::Ink));
        let c2: Preferences =
            serde_json::from_str(&serde_json::to_string(&c.prefs).unwrap()).unwrap();
        assert_eq!(c.prefs, c2);
        assert!(render(Path::new("a.txt"), "<hello>", &c).contains("#1d2025"));
    }
    #[test]
    fn missing_and_non_utf8_files_produce_readable_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("missing.md");
        assert!(String::from_utf8(
            load(
                url::Url::from_file_path(&p).unwrap().as_str(),
                &Config::default()
            )
            .unwrap()
        )
        .unwrap()
        .contains("role='alert'"));
        std::fs::write(&p, [255]).unwrap();
        assert!(String::from_utf8(
            load(
                url::Url::from_file_path(&p).unwrap().as_str(),
                &Config::default()
            )
            .unwrap()
        )
        .unwrap()
        .contains("not UTF-8"));
    }
}
