//! Reader mode, set like a broadsheet: the page's article, extracted in
//! the page and typeset here in Newsreader on our paper — no site CSS,
//! no site scripts, our measure, our rules. Ctrl+Shift+R toggles it over
//! a browser pane; the page keeps living underneath.

use crate::app::Caps;
use nus_render::text::{FontId, Style};
use nus_render::{Rect, Scene};

/// Runs in the page; returns JSON {title, byline, blocks:[{t,x,l,src}]}.
/// Picks the element with the most paragraph text, then walks it.
pub const EXTRACT_JS: &str = r##"(function(){
  function txt(e){return (e.innerText||e.textContent||'').replace(/\s+/g,' ').trim();}
  var cands=[].slice.call(document.querySelectorAll('article,main,[role=main],#content,#main,.post,.article,.entry-content,body'));
  var best=document.body,score=0;
  cands.forEach(function(c){var ps=c.querySelectorAll('p');var n=0;for(var i=0;i<ps.length;i++){n+=txt(ps[i]).length;}
    var s=n*(c.tagName==='BODY'?0.5:1);if(s>score){score=s;best=c;}});
  var blocks=[];var seen=0;
  function walk(e){
    if(!e||e.nodeType!==1)return;
    var tag=e.tagName;
    if(/^(SCRIPT|STYLE|NAV|ASIDE|FOOTER|HEADER|FORM|BUTTON|SVG|NOSCRIPT|IFRAME)$/.test(tag))return;
    var cs=getComputedStyle(e);if(cs.display==='none'||cs.visibility==='hidden')return;
    if(/^H[1-6]$/.test(tag)){var t=txt(e);if(t)blocks.push({t:'h',l:+tag[1],x:t});return;}
    if(tag==='P'){var t=txt(e);if(t.length>1){blocks.push({t:'p',x:t});seen+=t.length;}return;}
    if(tag==='PRE'){var t=(e.innerText||'').replace(/\s+$/,'');if(t)blocks.push({t:'pre',x:t});return;}
    if(tag==='LI'){var t=txt(e);if(t)blocks.push({t:'li',x:t});return;}
    if(tag==='BLOCKQUOTE'){var t=txt(e);if(t)blocks.push({t:'q',x:t});return;}
    if(tag==='IMG'){if((e.naturalWidth||e.width)>120)blocks.push({t:'img',x:e.alt||'',src:e.currentSrc||e.src||''});return;}
    if(tag==='FIGCAPTION'){var t=txt(e);if(t)blocks.push({t:'cap',x:t});return;}
    for(var i=0;i<e.children.length;i++)walk(e.children[i]);
  }
  walk(best);
  var m=function(n){var q=document.querySelector('meta[property="'+n+'"],meta[name="'+n+'"]');return q?q.content:'';};
  var h1=document.querySelector('h1');
  var title=(h1?txt(h1):'')||m('og:title')||document.title;
  var byline=m('author')||m('article:author')||'';
  var when=m('article:published_time')||m('date')||'';
  return JSON.stringify({title:title,byline:byline,when:when,blocks:blocks.slice(0,600)});
})()"##;

#[derive(Clone, Debug)]
pub enum Block {
    Heading(u8, String),
    Para(String),
    Pre(String),
    Item(String),
    Quote(String),
    Image(String, String),
    Caption(String),
}

#[derive(Clone, Debug, Default)]
pub struct Article {
    pub title: String,
    pub byline: String,
    pub when: String,
    pub blocks: Vec<Block>,
}

impl Article {
    pub fn parse(json: &str) -> Option<Article> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let mut blocks = Vec::new();
        for b in v.get("blocks").and_then(|b| b.as_array()).into_iter().flatten() {
            let x = b.get("x").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let t = b.get("t").and_then(|t| t.as_str()).unwrap_or("");
            blocks.push(match t {
                "h" => Block::Heading(b.get("l").and_then(|l| l.as_u64()).unwrap_or(2) as u8, x),
                "p" => Block::Para(x),
                "pre" => Block::Pre(x),
                "li" => Block::Item(x),
                "q" => Block::Quote(x),
                "img" => Block::Image(x, b.get("src").and_then(|s| s.as_str()).unwrap_or("").to_string()),
                "cap" => Block::Caption(x),
                _ => continue,
            });
        }
        Some(Article { title: s("title"), byline: s("byline"), when: s("when"), blocks })
    }

    /// Words, for the "ask about this page" hook and the header count.
    pub fn words(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| match b {
                Block::Heading(_, x) | Block::Para(x) | Block::Pre(x) | Block::Item(x) | Block::Quote(x) | Block::Caption(x) => x.split_whitespace().count(),
                Block::Image(..) => 0,
            })
            .sum()
    }

    pub fn plain(&self) -> String {
        let mut out = String::new();
        for b in &self.blocks {
            match b {
                Block::Heading(_, x) | Block::Para(x) | Block::Pre(x) | Block::Quote(x) | Block::Caption(x) => {
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
}

/// The reader over one page.
pub struct Reader {
    pub article: Article,
    pub scroll: f32,
    /// Layout cache keyed by (measure width, scale).
    pub lines: Vec<Line>,
    pub laid_for: (f32, f32),
    pub height: f32,
}

impl Reader {
    pub fn new(article: Article) -> Reader {
        Reader { article, scroll: 0.0, lines: Vec::new(), laid_for: (0.0, 0.0), height: 0.0 }
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

impl Reader {
    /// Typeset the article into lines for `width` (physical px) at `scale`.
    pub fn layout(&mut self, fonts: &nus_render::FontSystem, f: &ReaderFonts, width: f32, scale: f32, ink: nus_render::Color) {
        if self.laid_for == (width, scale) {
            return;
        }
        self.laid_for = (width, scale);
        self.lines.clear();
        let px = |v: f32| v * scale;
        let st = |font: FontId, size: f32| Style { font, px: px(size), color: ink, tracking: 0.0 };
        let body = st(f.serif, measure::BODY);
        let lead = px(measure::BODY * measure::LEADING);
        let mut y = px(8.0);
        let mut push = |kind: Kind, style: Style, text: &str, dx: f32, w: f32, leading: f32, y: &mut f32, lines: &mut Vec<Line>| {
            let size = style.px;
            for l in wrap(fonts, style, text, w - dx) {
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
        for b in &a.blocks {
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
                    for raw in x.lines() {
                        y += px(measure::MONO * 1.6);
                        self.lines.push(Line { kind: Kind::Mono, text: raw.replace('\t', "    "), dx: px(16.0), y });
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
                Block::Image(alt, _) => {
                    let text = if alt.is_empty() { "image".to_string() } else { format!("image · {alt}") };
                    push(Kind::Caption, st(f.mono, measure::CAPTION), &text.caps(), 0.0, width, px(measure::CAPTION * 1.6), &mut y, &mut self.lines);
                    y += px(10.0);
                }
                Block::Caption(x) => {
                    push(Kind::Caption, st(f.mono, measure::CAPTION), x, 0.0, width, px(measure::CAPTION * 1.6), &mut y, &mut self.lines);
                    y += px(10.0);
                }
            }
        }
        self.height = y + px(60.0);
    }

    /// Draw into `r`; the column is centred and never wider than the measure.
    pub fn draw(&mut self, scene: &mut Scene, fonts: &mut nus_render::FontSystem, f: &ReaderFonts, r: Rect, scale: f32, ink: nus_render::Color, dim: nus_render::Color, paper: nus_render::Color, signal: nus_render::Color) {
        let px = |v: f32| v * scale;
        let col_w = px(measure::COLUMN).min(r.w - 2.0 * px(measure::GUTTER));
        let x0 = (r.x + (r.w - col_w) / 2.0).round();
        self.layout(fonts, f, col_w, scale, ink);
        let max_scroll = (self.height - r.h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        scene.rect(r, paper);
        scene.layer(Some(r));
        let top = r.y + px(28.0) - self.scroll;
        for line in &self.lines {
            let y = top + line.y;
            if y < r.y - px(60.0) || y > r.bottom() + px(10.0) {
                continue;
            }
            let (font, size, color) = match line.kind {
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
            fonts.draw(scene, st, x0 + line.dx, y, &line.text);
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
