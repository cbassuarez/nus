//! Reader mode, set like a broadsheet: the page's article, extracted in
//! the page and typeset here in Newsreader on our paper — no site CSS,
//! no site scripts, our measure, our rules. Ctrl+Alt+R (⌘⌥R) toggles it over
//! a browser pane; the page keeps living underneath.

#[path = "library_reader.rs"]
pub(crate) mod interaction;

use crate::app::Caps;
use nus_render::text::{FontId, Style};
use nus_render::{Rect, Scene};

/// Shared, bounded read-only extraction for live pages and saved copies.
pub const EXTRACT_JS: &str = include_str!("../assets/library/article.js");

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Block {
    Heading(u8, String),
    Para(String),
    Pre(String),
    Item(String),
    Quote(String),
    Image(String, String),
    Caption(String),
    Link(String, String),
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Article {
    pub title: String,
    pub byline: String,
    pub when: String,
    pub blocks: Vec<Block>,
}

impl Article {
    pub fn parse(json: &str) -> Option<Article> {
        if json.len()>crate::library::store::MAX_TEXT {return None;}
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        if v.get("blocks").and_then(|b|b.as_array()).is_none_or(|b|b.len()>crate::library::store::MAX_BLOCKS){return None;}
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let mut blocks = Vec::new();
        for b in v.get("blocks").and_then(|b| b.as_array()).into_iter().flatten() {
            let x = b.get("x").and_then(|x| x.as_str())?.to_string();
            let t = b.get("t").and_then(|t| t.as_str())?;
            blocks.push(match t {
                "h" => Block::Heading(b.get("l").and_then(|l| l.as_u64()).unwrap_or(2).clamp(1, 6) as u8, x),
                "p" => Block::Para(x),
                "pre" => Block::Pre(x),
                "li" => Block::Item(x),
                "q" => Block::Quote(x),
                "img" => Block::Image(x, b.get("src").and_then(|s| s.as_str()).unwrap_or("").to_string()),
                "cap" => Block::Caption(x),
                "link" => Block::Link(x, b.get("src").and_then(|s|s.as_str()).unwrap_or("").into()),
                _ => return None,
            });
        }
        Some(Article { title: s("title"), byline: s("byline"), when: s("when"), blocks })
    }

    /// Words, for the "ask about this page" hook and the header count.
    pub fn words(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| match b {
                Block::Heading(_, x) | Block::Para(x) | Block::Pre(x) | Block::Item(x) | Block::Quote(x) | Block::Caption(x) | Block::Link(x, _) => x.split_whitespace().count(),
                Block::Image(..) => 0,
            })
            .sum()
    }

    pub fn plain(&self) -> String {
        let mut out = String::new();
        for b in &self.blocks {
            match b {
                Block::Heading(_, x) | Block::Para(x) | Block::Pre(x) | Block::Quote(x) | Block::Caption(x) | Block::Link(x, _) => {
                    out.push_str(x);
                    out.push_str("\n\n");
                }
                Block::Item(x) => {
                    out.push_str("· ");
                    out.push_str(x);
                    out.push('\n');
                }
                Block::Image(alt, _) => {
                    out.push_str(&format!("[image: {alt}]\n\n"));
                }
            }
        }
        out
    }
}

/// A typeset line: style kind, text, x offset, baseline y (from the top of
/// the article), and a rule/marker flag.
#[derive(Clone, Debug)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
    pub dx: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Title,
    Byline,
    H(u8),
    Body,
    Mono,
    Item,
    Quote,
    Caption,
    Rule,
    Link,
    Image,
    CodeAction,
}

/// The reader over one page.
pub struct Reader {
    pub article: Article,
    pub scroll: f32,
    /// Layout cache keyed by (measure width, scale).
    pub lines: Vec<Line>,
    pub laid_for: (f32, f32),
    pub height: f32,
    pub saved: interaction::Extras,
}

impl Reader {
    pub fn new(article: Article) -> Reader {
        Reader { article, scroll: 0.0, lines: Vec::new(), laid_for: (0.0, 0.0), height: 0.0, saved: Default::default() }
    }
}

/// Fonts the reader sets in.
pub struct ReaderFonts {
    pub serif: FontId,
    pub serif_italic: FontId,
    pub mono: FontId,
    pub mono_strong: FontId,
}

/// Type sizes (logical px) and leading, before scale.
pub mod measure {
    pub const BODY: f32 = 19.0;
    pub const LEADING: f32 = 1.5;
    pub const TITLE: f32 = 38.0;
    pub const H2: f32 = 26.0;
    pub const H3: f32 = 21.0;
    pub const MONO: f32 = 13.0;
    pub const CAPTION: f32 = 13.0;
    /// Measure: about 66 characters of Newsreader at BODY.
    pub const COLUMN: f32 = 640.0;
    pub const GUTTER: f32 = 48.0;
}

/// Greedy word wrap with the real font metrics.
pub fn wrap(fonts: &nus_render::FontSystem, style: Style, text: &str, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        if word.is_empty() {
            continue;
        }
        let trial = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if fonts.measure(style, &trial) <= width || line.is_empty() {
            line = trial;
        } else {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Shared by initial layout, reflow and the saved reader. Never negative on
/// narrow panes; unlike a fixed 48px gutter it leaves usable reading space.
pub fn column_width(width: f32, scale: f32) -> f32 {
    (measure::COLUMN * scale).min((width - 2.0 * (measure::GUTTER * scale).min(width * 0.08)).max(1.0))
}

/// Break overlong URLs/CJK runs inside the article only. The generic `wrap`
/// used by existing settings/welcome chrome is deliberately left unchanged.
fn wrap_article(fonts: &nus_render::FontSystem, style: Style, text: &str, width: f32) -> Vec<String> {
    let mut out = Vec::new();
    for line in wrap(fonts, style, text, width) {
        let mut rest = line.as_str();
        while !rest.is_empty() {
            // Bound each font-metric probe; repeatedly measuring the whole
            // remaining unbroken token would make an adversarial long URL slow.
            let ends: Vec<_> = rest.char_indices().map(|(i,c)| i + c.len_utf8()).take(256).collect();
            if ends.last().copied()==Some(rest.len()) && fonts.measure(style,rest)<=width {
                out.push(rest.to_string());break;
            }
            let (mut low, mut high) = (0usize, ends.len());
            while low < high {
                let mid = (low + high + 1) / 2;
                if fonts.measure(style, &rest[..ends[mid-1]]) <= width { low = mid; } else { high = mid-1; }
            }
            let end = ends[low.max(1)-1];
            out.push(rest[..end].to_string()); rest = &rest[end..];
        }
    }
    out
}

impl Reader {
    /// Typeset the article into lines for `width` (physical px) at `scale`.
    pub fn layout(&mut self, fonts: &nus_render::FontSystem, f: &ReaderFonts, width: f32, scale: f32, ink: nus_render::Color) {
        let font_key=[f.serif,f.serif_italic,f.mono,f.mono_strong];
        if self.laid_for == (width, scale) && self.saved.font_key==Some(font_key) {
            return;
        }
        self.laid_for = (width, scale);
        self.lines.clear();
        self.saved.reset_layout();
        self.saved.font_key=Some(font_key);
        let px = |v: f32| v * scale;
        let st = |font: FontId, size: f32| Style { font, px: px(size), color: ink, tracking: 0.0 };
        let body = st(f.serif, measure::BODY);
        let lead = px(measure::BODY * measure::LEADING);
        let mut y = px(8.0);
        let mut push = |kind: Kind, style: Style, text: &str, dx: f32, w: f32, leading: f32, y: &mut f32, lines: &mut Vec<Line>| {
            let size = style.px;
            for l in wrap_article(fonts, style, text, (w - dx).max(1.0)) {
                *y += leading.max(size * 1.15);
                lines.push(Line { kind, text: l, dx, y: *y });
            }
        };
        // Masthead.
        let a = &self.article;
        if !a.title.is_empty() {
            push(Kind::Title, st(f.serif, measure::TITLE), &a.title, 0.0, width, px(measure::TITLE * 1.12), &mut y, &mut self.lines);
            y += px(6.0);
        }
        let mut by = a.byline.clone();
        if !a.when.is_empty() {
            let when = a.when.split('T').next().unwrap_or("").to_string();
            if !when.is_empty() {
                by = if by.is_empty() { when } else { format!("{by} · {when}") };
            }
        }
        if !by.is_empty() {
            push(Kind::Byline, st(f.serif_italic, measure::BODY), &by, 0.0, width, lead, &mut y, &mut self.lines);
        }
        y += px(10.0);
        self.lines.push(Line { kind: Kind::Rule, text: String::new(), dx: 0.0, y });
        y += px(18.0);
        for (block_index,b) in a.blocks.iter().enumerate() {
            let first=self.lines.len();
            match b {
                Block::Heading(l, x) => {
                    y += px(12.0);
                    let (size, kind) = if *l <= 2 { (measure::H2, Kind::H(2)) } else { (measure::H3, Kind::H(3)) };
                    push(kind, st(f.serif, size), x, 0.0, width, px(size * 1.2), &mut y, &mut self.lines);
                    y += px(4.0);
                }
                Block::Para(x) => {
                    push(Kind::Body, body, x, 0.0, width, lead, &mut y, &mut self.lines);
                    y += px(12.0);
                }
                Block::Pre(x) => {
                    let mono = st(f.mono, measure::MONO);
                    y += px(6.0);
                    if self.saved.offline {
                        y+=px(24.0);
                        self.saved.code.insert(self.lines.len(),x.clone());
                        self.lines.push(Line{kind:Kind::CodeAction,text:"COPY CODE / TABLE".into(),dx:0.0,y});
                    }
                    for raw in x.lines() {
                        y += px(measure::MONO * 1.6);
                        let text=raw.replace('\t',"    ");
                        self.saved.horizontal_max=self.saved.horizontal_max.max((fonts.measure(mono,&text)+px(32.0)-width).max(0.0));
                        self.lines.push(Line { kind: Kind::Mono, text, dx: px(16.0), y });
                    }
                    y += px(14.0);
                    let _ = mono;
                }
                Block::Item(x) => {
                    push(Kind::Item, body, x, px(24.0), width, lead, &mut y, &mut self.lines);
                    y += px(4.0);
                }
                Block::Quote(x) => {
                    push(Kind::Quote, st(f.serif_italic, measure::BODY), x, px(24.0), width, lead, &mut y, &mut self.lines);
                    y += px(12.0);
                }
                Block::Image(alt, id) => {
                    if let Some(pic)=self.saved.pictures.get(id) {
                        let image_w=width.min(pic.width as f32*scale);
                        let image_h=image_w*pic.height as f32/pic.width.max(1) as f32;
                        self.saved.image_heights.insert(self.lines.len(),image_h);
                        self.lines.push(Line{kind:Kind::Image,text:id.clone(),dx:(width-image_w)/2.0,y});
                        y+=image_h+px(12.0);
                    }
                    let noun=if self.saved.offline && !self.saved.pictures.contains_key(id){"image unavailable offline"}else{"image"};
                    let text = if alt.is_empty() { noun.to_string() } else { format!("{noun} · {alt}") };
                    push(Kind::Caption, st(f.mono, measure::CAPTION), &text.caps(), 0.0, width, px(measure::CAPTION * 1.6), &mut y, &mut self.lines);
                    y += px(10.0);
                }
                Block::Link(x,url) => {
                    let before=self.lines.len();
                    let label=format!("{x} · {url}");
                    push(Kind::Link,st(f.serif,measure::BODY),&label,0.0,width,lead,&mut y,&mut self.lines);
                    for i in before..self.lines.len(){self.saved.links.insert(i,url.clone());}
                    y+=px(10.0);
                }
                Block::Caption(x) => {
                    push(Kind::Caption, st(f.mono, measure::CAPTION), x, 0.0, width, px(measure::CAPTION * 1.6), &mut y, &mut self.lines);
                    y += px(10.0);
                }
            }
            self.saved.blocks.push((first,self.lines.len(),block_index));
        }
        self.height = y + px(60.0);
    }

    /// Draw into `r`; the column is centred and never wider than the measure.
    pub fn draw(&mut self, scene: &mut Scene, fonts: &mut nus_render::FontSystem, f: &ReaderFonts, r: Rect, scale: f32, ink: nus_render::Color, dim: nus_render::Color, paper: nus_render::Color, signal: nus_render::Color) {
        let px = |v: f32| v * scale;
        let col_w = column_width(r.w, scale);
        let x0 = (r.x + (r.w - col_w) / 2.0).round();
        self.layout(fonts, f, col_w, scale, ink);
        let max_scroll = (self.height - r.h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        scene.rect(r, paper);
        scene.layer(Some(r));
        self.saved.viewport=Some(r); self.saved.scale=scale;
        self.saved.hits.clear(); self.saved.drawn.clear();
        self.saved.horizontal=self.saved.horizontal.clamp(0.0,self.saved.horizontal_max);
        let top = r.y + px(28.0) - self.scroll;
        for (index,line) in self.lines.iter().enumerate() {
            let y = top + line.y;
            if line.kind==Kind::Image {
                if let (Some(pic),Some(&height))=(self.saved.pictures.get(&line.text),self.saved.image_heights.get(&index)) {
                    if y+height>=r.y && y<=r.bottom() {scene.texture(Rect::new(x0+line.dx,y,col_w-2.0*line.dx,height),pic.texture.clone(),Some(r));}
                }
                continue;
            }
            if y < r.y - px(60.0) || y > r.bottom() + px(10.0) {
                continue;
            }
            let (font, size, color) = match line.kind {
                Kind::Image => continue,
                Kind::Link => (f.serif, measure::BODY, signal),
                Kind::CodeAction => (f.mono_strong, measure::CAPTION, ink),
                Kind::Title => (f.serif, measure::TITLE, ink),
                Kind::Byline => (f.serif_italic, measure::BODY, dim),
                Kind::H(2) => (f.serif, measure::H2, ink),
                Kind::H(_) => (f.serif, measure::H3, ink),
                Kind::Body | Kind::Item => (f.serif, measure::BODY, ink),
                Kind::Quote => (f.serif_italic, measure::BODY, ink),
                Kind::Mono => (f.mono, measure::MONO, ink),
                Kind::Caption => (f.mono, measure::CAPTION, dim),
                Kind::Rule => {
                    scene.rect(Rect::new(x0, y, col_w, px(2.0)), ink);
                    continue;
                }
            };
            if line.kind == Kind::Item && line.dx > 0.0 {
                scene.rect(Rect::new(x0 + px(6.0), y - px(7.0), px(5.0), px(5.0)), signal);
            }
            if line.kind == Kind::Quote {
                scene.rect(Rect::new(x0 + px(4.0), y - px(measure::BODY), px(2.0), px(measure::BODY * measure::LEADING)), ink);
            }
            let st = Style { font, px: px(size), color, tracking: 0.0 };
            let x=x0+line.dx-if line.kind==Kind::Mono {self.saved.horizontal}else{0.0};
            let text_w=fonts.measure(st,&line.text);
            let hit=Rect::new(x,y-st.px*1.05,text_w.max(px(6.0)),st.px*1.45).intersect(&r);
            if self.saved.found==Some(index) {scene.rect(Rect::new(x,y-st.px*1.05,text_w,st.px*1.45),[signal[0],signal[1],signal[2],0.16]);}
            if let Some((a,b))=self.saved.selection {
                let(a,b)=if a<=b{(a,b)}else{(b,a)};
                if index>=a.line && index<=b.line {
                    let from=if index==a.line{a.byte}else{0};let to=if index==b.line{b.byte}else{line.text.len()};
                    if let (Some(prefix),Some(selected))=(line.text.get(..from),line.text.get(from..to)) {
                        let dx=fonts.measure(st,prefix);let w=fonts.measure(st,selected);
                        scene.rect(Rect::new(x+dx,y-st.px*1.05,w,st.px*1.45),[signal[0],signal[1],signal[2],0.24]);
                    }
                }
            }
            fonts.draw(scene, st, x, y, &line.text);
            if hit.w>0.0 && hit.h>0.0 {
                if line.kind==Kind::CodeAction {
                    scene.hline(x,y+px(3.0),text_w,px(1.0),ink);
                    self.saved.hits.push((hit,interaction::Hit::Code(index)));
                } else {
                    self.saved.drawn.push((index,Rect::new(x,y-st.px*1.05,text_w,st.px*1.45),st));
                    if let Some(url)=self.saved.links.get(&index).filter(|_|self.saved.offline) {
                        scene.hline(x,y+px(3.0),text_w,px(1.0),signal);
                        self.saved.hits.push((hit,interaction::Hit::Link(url.clone())));
                    }
                }
            }
        }
        scene.layer(None);
        // A hairline scroll track on the right, ink for the visible part.
        if max_scroll > 0.0 {
            let track = Rect::new(r.right() - px(6.0), r.y + px(8.0), px(2.0), r.h - px(16.0));
            let frac = (r.h / self.height).clamp(0.05, 1.0);
            let pos = self.scroll / max_scroll;
            scene.rect(Rect::new(track.x, track.y + (track.h - track.h * frac) * pos, track.w, track.h * frac), dim);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_blocks() {
        let a = Article::parse(r#"{"title":"T","byline":"B","when":"2026-09-16T00:00:00Z","blocks":[{"t":"h","l":2,"x":"H"},{"t":"p","x":"one two"},{"t":"img","x":"alt","src":"u"}]}"#).unwrap();
        assert_eq!(a.blocks.len(), 3);
        assert_eq!(a.words(), 3);
        assert!(a.plain().contains("[image: alt]"));
    }
}
