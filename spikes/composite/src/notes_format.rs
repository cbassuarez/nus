//! Formatting a note: what its Markdown looks like while you write it, and
//! the edits the rail and the keys make. All of it is text in, text out:
//! a style is drawn over the characters that are there (markers stay,
//! dimmed), and an edit is one splice of the note's own Markdown, so undo
//! takes it back in one step and nothing you did not touch is rewritten.

/// How a run of characters is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Plain,
    /// `#`, `**`, backticks, `>`, `==`: there, and faint.
    Marker,
    Heading(u8),
    Bold,
    Italic,
    Code,
    /// A link's words; its address is `Url`.
    Link,
    Url,
    Mark,
    Quote,
    /// `-`, `1.`: the list's own marker.
    Bullet,
    /// `[ ]` or `[x]`.
    Box { checked: bool },
    /// A ticked item's words.
    Done,
    /// A fence's ``` line, and the code inside it.
    Fence,
    FenceBody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub len: usize,
    pub style: Style,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Text,
    FenceEdge,
    FenceBody,
}

/// Which lines are inside a fenced code block (and which are its edges).
pub fn line_kinds<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<LineKind> {
    let mut out = Vec::new();
    let mut open: Option<(char, usize)> = None;
    for l in lines {
        let t = l.trim_start();
        let c = t.chars().next().filter(|c| *c == '`' || *c == '~');
        let n = c.map_or(0, |c| t.chars().take_while(|x| *x == c).count());
        match open {
            Some((fc, fn_)) => {
                if c == Some(fc) && n >= fn_ && t.trim_end().chars().all(|x| x == fc) {
                    open = None;
                    out.push(LineKind::FenceEdge);
                } else {
                    out.push(LineKind::FenceBody);
                }
            }
            None if n >= 3 => {
                open = Some((c.unwrap_or('`'), n));
                out.push(LineKind::FenceEdge);
            }
            None => out.push(LineKind::Text),
        }
    }
    out
}

/// The block a line is: its indent, marker and what kind of item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub indent: usize,
    /// Characters of marker after the indent (`## `, `- [ ] `, `12. `).
    pub marker_len: usize,
    pub kind: BlockKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Para,
    Heading(u8),
    Quote,
    Bullet(char),
    Number(u32),
    Check { checked: bool, bullet: char },
}

pub fn block(line: &str) -> Block {
    let chars: Vec<char> = line.chars().collect();
    let indent = chars.iter().take_while(|c| **c == ' ' || **c == '\t').count();
    let rest: String = chars[indent..].iter().collect();
    let hashes = rest.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && rest.chars().nth(hashes) == Some(' ') {
        return Block { indent, marker_len: hashes + 1, kind: BlockKind::Heading(hashes as u8) };
    }
    if rest.starts_with("> ") || rest == ">" {
        return Block { indent, marker_len: rest.len().min(2), kind: BlockKind::Quote };
    }
    let first = rest.chars().next();
    if let Some(b @ ('-' | '*' | '+')) = first {
        if rest.chars().nth(1) == Some(' ') {
            let after = &rest[2..];
            for (mark, checked) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
                if after.starts_with(mark) || after == mark.trim_end() {
                    return Block { indent, marker_len: 2 + mark.len().min(after.len().max(3)), kind: BlockKind::Check { checked, bullet: b } };
                }
            }
            return Block { indent, marker_len: 2, kind: BlockKind::Bullet(b) };
        }
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if (1..=9).contains(&digits) && matches!(rest.chars().nth(digits), Some('.' | ')')) && rest.chars().nth(digits + 1) == Some(' ') {
        let n = rest[..digits].parse().unwrap_or(1);
        return Block { indent, marker_len: digits + 2, kind: BlockKind::Number(n) };
    }
    Block { indent, marker_len: 0, kind: BlockKind::Para }
}

/// How one line is drawn (chars, by column). Runs not covered are plain.
pub fn styles(line: &str, kind: LineKind) -> Vec<Span> {
    let n = line.chars().count();
    match kind {
        LineKind::FenceEdge => return vec![Span { start: 0, len: n, style: Style::Fence }],
        LineKind::FenceBody => return vec![Span { start: 0, len: n, style: Style::FenceBody }],
        LineKind::Text => {}
    }
    let b = block(line);
    let mut out = Vec::new();
    let body_at = b.indent + b.marker_len;
    let base = match b.kind {
        BlockKind::Heading(l) => {
            out.push(Span { start: b.indent, len: b.marker_len, style: Style::Marker });
            Style::Heading(l)
        }
        BlockKind::Quote => {
            out.push(Span { start: b.indent, len: b.marker_len, style: Style::Marker });
            Style::Quote
        }
        BlockKind::Bullet(_) | BlockKind::Number(_) => {
            out.push(Span { start: b.indent, len: b.marker_len, style: Style::Bullet });
            Style::Plain
        }
        BlockKind::Check { checked, .. } => {
            out.push(Span { start: b.indent, len: 2, style: Style::Bullet });
            out.push(Span { start: b.indent + 2, len: 3.min(b.marker_len - 2), style: Style::Box { checked } });
            if checked { Style::Done } else { Style::Plain }
        }
        BlockKind::Para => Style::Plain,
    };
    let chars: Vec<char> = line.chars().collect();
    inline(&chars, body_at.min(n), n, base, &mut out);
    out.sort_by_key(|s| s.start);
    out
}

fn find(chars: &[char], from: usize, to: usize, pat: &[char]) -> Option<usize> {
    if pat.is_empty() || to < pat.len() {
        return None;
    }
    (from..=to - pat.len()).find(|&i| chars[i..i + pat.len()] == *pat)
}

/// Inline runs from `from` to `to`, with `base` where nothing else holds.
fn inline(chars: &[char], from: usize, to: usize, base: Style, out: &mut Vec<Span>) {
    let mut i = from;
    let mut plain_from = from;
    let flush = |out: &mut Vec<Span>, a: usize, z: usize| {
        if z > a && base != Style::Plain {
            out.push(Span { start: a, len: z - a, style: base });
        }
    };
    while i < to {
        let c = chars[i];
        // Code first: nothing inside it is formatting.
        if c == '`' {
            let ticks = chars[i..to].iter().take_while(|x| **x == '`').count();
            let pat = vec!['`'; ticks];
            if let Some(end) = find(chars, i + ticks, to, &pat) {
                flush(out, plain_from, i);
                out.push(Span { start: i, len: ticks, style: Style::Marker });
                out.push(Span { start: i + ticks, len: end - i - ticks, style: Style::Code });
                out.push(Span { start: end, len: ticks, style: Style::Marker });
                i = end + ticks;
                plain_from = i;
                continue;
            }
            i += ticks;
            continue;
        }
        let pair = |d: &[char], style: Style, out: &mut Vec<Span>, i: &mut usize, plain_from: &mut usize| -> bool {
            let m = d.len();
            if *i + m >= to || chars[*i..*i + m] != *d || chars[*i + m].is_whitespace() {
                return false;
            }
            let Some(end) = find(chars, *i + m + 1, to, d) else { return false };
            if chars[end - 1].is_whitespace() {
                return false;
            }
            // `_` inside a word (snake_case) is not emphasis.
            if d[0] == '_' && (*i > 0 && chars[*i - 1].is_alphanumeric() || chars.get(end + m).is_some_and(|c| c.is_alphanumeric())) {
                return false;
            }
            flush(out, *plain_from, *i);
            out.push(Span { start: *i, len: m, style: Style::Marker });
            out.push(Span { start: *i + m, len: end - *i - m, style });
            out.push(Span { start: end, len: m, style: Style::Marker });
            *i = end + m;
            *plain_from = *i;
            true
        };
        if pair(&['*', '*'], Style::Bold, out, &mut i, &mut plain_from)
            || pair(&['_', '_'], Style::Bold, out, &mut i, &mut plain_from)
            || pair(&['=', '='], Style::Mark, out, &mut i, &mut plain_from)
            || pair(&['*'], Style::Italic, out, &mut i, &mut plain_from)
            || pair(&['_'], Style::Italic, out, &mut i, &mut plain_from)
        {
            continue;
        }
        if c == '[' {
            if let Some(close) = find(chars, i + 1, to, &[']', '(']) {
                if let Some(paren) = find(chars, close + 2, to, &[')']) {
                    flush(out, plain_from, i);
                    out.push(Span { start: i, len: 1, style: Style::Marker });
                    out.push(Span { start: i + 1, len: close - i - 1, style: Style::Link });
                    out.push(Span { start: close, len: paren + 1 - close, style: Style::Url });
                    i = paren + 1;
                    plain_from = i;
                    continue;
                }
            }
        }
        i += 1;
    }
    flush(out, plain_from, to);
}

/// What the caret's place already is, for the rail to light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Active {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub mark: bool,
    pub link: bool,
    pub heading: u8,
    pub bullet: bool,
    pub number: bool,
    pub check: bool,
    pub quote: bool,
}

pub fn active(line: &str, kind: LineKind, col: usize) -> Active {
    let mut a = Active::default();
    if kind != LineKind::Text {
        a.code = true;
        return a;
    }
    match block(line).kind {
        BlockKind::Heading(l) => a.heading = l,
        BlockKind::Quote => a.quote = true,
        BlockKind::Bullet(_) => a.bullet = true,
        BlockKind::Number(_) => a.number = true,
        BlockKind::Check { .. } => a.check = true,
        BlockKind::Para => {}
    }
    for s in styles(line, kind) {
        // Inside the run or right at its end (typing on).
        if col >= s.start && col <= s.start + s.len && s.len > 0 {
            match s.style {
                Style::Bold => a.bold = true,
                Style::Italic => a.italic = true,
                Style::Code => a.code = true,
                Style::Mark => a.mark = true,
                Style::Link => a.link = true,
                _ => {}
            }
        }
    }
    a
}

/// One splice: replace chars `start..end` with `insert`, then select
/// `anchor..cursor` (char indices after the splice).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub insert: String,
    pub anchor: Option<usize>,
    pub cursor: usize,
}

/// Bold, italic, code or a highlighter mark around the selection (or the
/// word at the caret), or taken off when it is already there. `text` is
/// the caret's surroundings; positions are relative to it.
pub fn toggle_inline(text: &str, sel: (usize, usize), marker: &str) -> Edit {
    let chars: Vec<char> = text.chars().collect();
    let m: Vec<char> = marker.chars().collect();
    let k = m.len();
    let (mut a, mut z) = (sel.0.min(sel.1), sel.0.max(sel.1));
    // No selection: the word at the caret.
    if a == z {
        let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
        while a > 0 && word(chars[a - 1]) {
            a -= 1;
        }
        while z < chars.len() && word(chars[z]) {
            z += 1;
        }
    }
    let s = |x: usize, y: usize| chars[x..y].iter().collect::<String>();
    // Already wrapped just outside: unwrap.
    if a >= k && z + k <= chars.len() && chars[a - k..a] == *m && chars[z..z + k] == *m {
        let inner = s(a, z);
        let len = inner.chars().count();
        return Edit { start: a - k, end: z + k, insert: inner, anchor: (len > 0).then_some(a - k), cursor: a - k + len };
    }
    // The selection includes its markers: unwrap.
    if z - a >= 2 * k && chars[a..a + k] == *m && chars[z - k..z] == *m {
        let inner = s(a + k, z - k);
        let len = inner.chars().count();
        return Edit { start: a, end: z, insert: inner, anchor: Some(a), cursor: a + len };
    }
    let inner = s(a, z);
    let len = inner.chars().count();
    if len == 0 {
        // Nothing to wrap: a pair, the caret between.
        return Edit { start: a, end: z, insert: format!("{marker}{marker}"), anchor: None, cursor: a + k };
    }
    Edit { start: a, end: z, insert: format!("{marker}{inner}{marker}"), anchor: Some(a + k), cursor: a + k + len }
}

/// A link around the selection: `[words](https://)` with the address
/// selected to type over.
pub fn link(text: &str, sel: (usize, usize)) -> Edit {
    let chars: Vec<char> = text.chars().collect();
    let (a, z) = (sel.0.min(sel.1), sel.0.max(sel.1));
    let words: String = chars[a..z].iter().collect();
    let looks_url = words.starts_with("http://") || words.starts_with("https://");
    if looks_url {
        let insert = format!("[]({words})");
        return Edit { start: a, end: z, insert, anchor: None, cursor: a + 1 };
    }
    let n = words.chars().count();
    let insert = format!("[{words}](https://)");
    Edit { start: a, end: z, insert, anchor: Some(a + n + 3), cursor: a + n + 11 }
}

/// What a line tool makes of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineTool {
    Heading(u8),
    Bullet,
    Number,
    Check,
    Quote,
}

/// Apply a line tool to these lines: when every line already is that,
/// take it off; else make each one that (replacing another list or
/// heading marker, keeping indent and words). Numbers count from 1.
pub fn toggle_lines(lines: &[&str], tool: LineTool) -> Vec<String> {
    let is = |b: &Block| match (tool, b.kind) {
        (LineTool::Heading(l), BlockKind::Heading(h)) => l == h,
        (LineTool::Bullet, BlockKind::Bullet(_)) => true,
        (LineTool::Number, BlockKind::Number(_)) => true,
        (LineTool::Check, BlockKind::Check { .. }) => true,
        (LineTool::Quote, BlockKind::Quote) => true,
        _ => false,
    };
    let blocks: Vec<Block> = lines.iter().map(|l| block(l)).collect();
    let content: Vec<&str> = lines.iter().filter(|l| !l.trim().is_empty()).copied().collect();
    let all = !content.is_empty() && lines.iter().zip(&blocks).filter(|(l, _)| !l.trim().is_empty()).all(|(_, b)| is(b));
    let mut n = 0;
    lines.iter().zip(&blocks).map(|(l, b)| {
        let chars: Vec<char> = l.chars().collect();
        if l.trim().is_empty() && lines.len() > 1 {
            return l.to_string();
        }
        let indent: String = chars[..b.indent].iter().collect();
        let words: String = chars[(b.indent + b.marker_len).min(chars.len())..].iter().collect();
        if all {
            return format!("{indent}{words}");
        }
        n += 1;
        let marker = match tool {
            LineTool::Heading(h) => format!("{} ", "#".repeat(h as usize)),
            LineTool::Bullet => "- ".into(),
            LineTool::Number => format!("{n}. "),
            LineTool::Check => "- [ ] ".into(),
            LineTool::Quote => "> ".into(),
        };
        // A heading is never indented; list items keep theirs.
        let indent = if matches!(tool, LineTool::Heading(_)) { String::new() } else { indent };
        format!("{indent}{marker}{words}")
    }).collect()
}

/// Enter at the end of a list item: what goes after the newline (the next
/// marker), or, on an empty item, the item's marker taken away instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Enter {
    /// Not a list: the editor's own newline.
    Plain,
    /// Insert `\n` and this.
    Continue(String),
    /// The item was empty: clear the line to its indent, no newline.
    End,
}

pub fn enter(line: &str) -> Enter {
    let b = block(line);
    let indent: String = line.chars().take(b.indent).collect();
    let empty = line.chars().skip(b.indent + b.marker_len).all(char::is_whitespace);
    let next = match b.kind {
        BlockKind::Bullet(c) => format!("{indent}{c} "),
        BlockKind::Number(n) => format!("{indent}{}{} ", n + 1, line.chars().nth(b.indent + n.to_string().len()).unwrap_or('.')),
        BlockKind::Check { bullet, .. } => format!("{indent}{bullet} [ ] "),
        BlockKind::Quote => "> ".into(),
        _ => return Enter::Plain,
    };
    if empty { Enter::End } else { Enter::Continue(next) }
}

/// Numbered items from `lines[from]` on, at its indent, counted again from
/// its own number: (line index, new text) for the lines that change.
pub fn renumber(lines: &[&str], from: usize) -> Vec<(usize, String)> {
    let Some(first) = lines.get(from) else { return Vec::new() };
    let b0 = block(first);
    let BlockKind::Number(mut n) = b0.kind else { return Vec::new() };
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate().skip(from + 1) {
        let b = block(l);
        if b.indent > b0.indent && !matches!(b.kind, BlockKind::Para) {
            continue; // a nested list keeps its own numbers
        }
        let BlockKind::Number(m) = b.kind else { break };
        if b.indent != b0.indent {
            break;
        }
        n += 1;
        if m != n {
            let chars: Vec<char> = l.chars().collect();
            let digits = m.to_string().len();
            let rest: String = chars[b.indent + digits..].iter().collect();
            let indent: String = chars[..b.indent].iter().collect();
            out.push((i, format!("{indent}{n}{rest}")));
        }
    }
    out
}

/// A note's open checklist items, outside code: (line, the line as written,
/// the item's words).
pub fn open_tasks(body: &str) -> Vec<(usize, String, String)> {
    let lines: Vec<&str> = body.lines().collect();
    let kinds = line_kinds(lines.iter().copied());
    lines.iter().zip(kinds).enumerate().filter_map(|(i, (l, k))| {
        if k != LineKind::Text {
            return None;
        }
        let b = block(l);
        if !matches!(b.kind, BlockKind::Check { checked: false, .. }) {
            return None;
        }
        let words: String = l.chars().skip(b.indent + b.marker_len).collect();
        let words = words.trim();
        (!words.is_empty()).then(|| (i, l.to_string(), words.to_string()))
    }).collect()
}

/// Where the link at a column goes: a Markdown link's address (the column
/// on its words or its address), else a bare http(s) address there.
pub fn link_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if chars[i] == '[' {
            if let Some(close) = find(&chars, i + 1, n, &[']', '(']) {
                if let Some(paren) = find(&chars, close + 2, n, &[')']) {
                    if (i..=paren).contains(&col) {
                        return Some(chars[close + 2..paren].iter().collect());
                    }
                    i = paren + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    let col = col.min(n);
    let mut a = col;
    while a > 0 && !chars[a - 1].is_whitespace() {
        a -= 1;
    }
    let mut z = col;
    while z < n && !chars[z].is_whitespace() {
        z += 1;
    }
    let word: String = chars[a..z].iter().collect();
    let w = word.trim_start_matches(['(', '<']).trim_end_matches(['.', ',', ';', ')', '>']);
    (w.starts_with("http://") || w.starts_with("https://")).then(|| w.to_string())
}

/// Tick or untick a checklist item: the column of the character to change
/// and what it becomes.
pub fn toggle_box(line: &str) -> Option<(usize, char)> {
    let b = block(line);
    match b.kind {
        BlockKind::Check { checked, .. } => Some((b.indent + 3, if checked { ' ' } else { 'x' })),
        _ => None,
    }
}

/// What the rail's buttons, the keys and the palette do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Bold,
    Italic,
    Code,
    Link,
    Mark,
    Heading(u8),
    Bullet,
    Number,
    Check,
    Quote,
}

/// The rail, top to bottom: inline first, then lines.
pub const RAIL: [Act; 12] = [Act::Bold, Act::Italic, Act::Code, Act::Link, Act::Mark, Act::Heading(1), Act::Heading(2), Act::Heading(3), Act::Bullet, Act::Number, Act::Check, Act::Quote];

impl Act {
    /// The button's face.
    pub fn face(self) -> &'static str {
        match self {
            Act::Bold => "B",
            Act::Italic => "I",
            Act::Code => "</>",
            Act::Link => "↗",
            Act::Mark => "==",
            Act::Heading(1) => "H1",
            Act::Heading(2) => "H2",
            Act::Heading(_) => "H3",
            Act::Bullet => "•",
            Act::Number => "1.",
            Act::Check => "☐",
            Act::Quote => "❝",
        }
    }

    /// Its name, and its keys (⌘⌥, Ctrl+Alt off macOS).
    pub fn name(self) -> (&'static str, &'static str) {
        match self {
            Act::Bold => ("bold", "B"),
            Act::Italic => ("italic", "I"),
            Act::Code => ("code", "E"),
            Act::Link => ("link", "K"),
            Act::Mark => ("highlight", "M"),
            Act::Heading(1) => ("heading 1", "1"),
            Act::Heading(2) => ("heading 2", "2"),
            Act::Heading(_) => ("heading 3", "3"),
            Act::Bullet => ("bulleted list", "7"),
            Act::Number => ("numbered list", "8"),
            Act::Check => ("checklist", "9"),
            Act::Quote => ("quote", "'"),
        }
    }

    pub fn lit(self, a: &Active) -> bool {
        match self {
            Act::Bold => a.bold,
            Act::Italic => a.italic,
            Act::Code => a.code,
            Act::Link => a.link,
            Act::Mark => a.mark,
            Act::Heading(h) => a.heading == h,
            Act::Bullet => a.bullet,
            Act::Number => a.number,
            Act::Check => a.check,
            Act::Quote => a.quote,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_tasks_are_unticked_items_outside_code() {
        let body = "# Plan\n- [ ] write it\n- [x] done\n  * [ ] nested one\n```\n- [ ] not a task\n```\n- [ ] \n1. [ ] numbered is not a box";
        let t = open_tasks(body);
        assert_eq!(t.iter().map(|(l, _, w)| (*l, w.as_str())).collect::<Vec<_>>(), vec![(1, "write it"), (3, "nested one")]);
        assert_eq!(t[1].1, "  * [ ] nested one");
    }

    #[test]
    fn link_at_finds_the_address_from_words_or_address() {
        let l = "see [the doc](https://x.dev/a) and note:abc";
        assert_eq!(link_at(l, 6).as_deref(), Some("https://x.dev/a"));
        assert_eq!(link_at(l, 20).as_deref(), Some("https://x.dev/a"));
        assert_eq!(link_at(l, 1), None);
        assert_eq!(link_at("[n](note:k1) after", 1).as_deref(), Some("note:k1"));
        assert_eq!(link_at("go to https://a.b/c, then", 9).as_deref(), Some("https://a.b/c"));
        assert_eq!(link_at("é [ü](https://ü.de) x", 3).as_deref(), Some("https://ü.de"));
    }

    fn kinds(line: &str) -> Vec<(String, Style)> {
        let chars: Vec<char> = line.chars().collect();
        styles(line, LineKind::Text).into_iter().map(|s| (chars[s.start..s.start + s.len].iter().collect(), s.style)).collect()
    }

    #[test]
    fn inline_styles_keep_their_markers() {
        let k = kinds("a **bold** and _it_ and `c **d**` and ==m== [w](u)");
        assert!(k.contains(&("bold".into(), Style::Bold)));
        assert!(k.contains(&("it".into(), Style::Italic)));
        assert!(k.contains(&("c **d**".into(), Style::Code)), "nothing is formatting inside code");
        assert!(k.contains(&("m".into(), Style::Mark)));
        assert!(k.contains(&("w".into(), Style::Link)));
        assert!(k.contains(&("](u)".into(), Style::Url)));
        assert!(k.contains(&("**".into(), Style::Marker)));
        assert!(!kinds("snake_case_name").iter().any(|(_, s)| *s == Style::Italic));
        assert!(!kinds("a * b * c").iter().any(|(_, s)| *s == Style::Italic));
    }

    #[test]
    fn blocks_headings_lists_and_boxes() {
        assert_eq!(block("## Title").kind, BlockKind::Heading(2));
        assert_eq!(block("#nope").kind, BlockKind::Para);
        assert_eq!(block("  - item").kind, BlockKind::Bullet('-'));
        assert_eq!(block("12. item").kind, BlockKind::Number(12));
        assert_eq!(block("- [x] done").kind, BlockKind::Check { checked: true, bullet: '-' });
        assert_eq!(block("> said").kind, BlockKind::Quote);
        let k = kinds("- [x] done");
        assert!(k.contains(&("[x]".into(), Style::Box { checked: true })));
        assert!(k.contains(&("done".into(), Style::Done)));
        let h = kinds("# Big **deal**");
        assert!(h.contains(&("Big ".into(), Style::Heading(1))));
        assert!(h.contains(&("deal".into(), Style::Bold)));
    }

    #[test]
    fn fences_are_code_all_the_way_through() {
        let k = line_kinds(["a", "```sh", "# not a heading", "````", "b"].into_iter());
        assert_eq!(k, vec![LineKind::Text, LineKind::FenceEdge, LineKind::FenceBody, LineKind::FenceEdge, LineKind::Text], "a longer fence closes a shorter one");
        let k = line_kinds(["````md", "```", "inner", "```", "````"].into_iter());
        assert_eq!(k[2], LineKind::FenceBody, "a shorter fence does not close a longer one");
        assert_eq!(k[4], LineKind::FenceEdge);
    }

    #[test]
    fn inline_toggles_wrap_and_unwrap() {
        let e = toggle_inline("say hello now", (4, 9), "**");
        assert_eq!((e.start, e.end, e.insert.as_str(), e.anchor, e.cursor), (4, 9, "**hello**", Some(6), 11));
        let again = toggle_inline("say **hello** now", (6, 11), "**");
        assert_eq!((again.start, again.end, again.insert.as_str()), (4, 13, "hello"));
        let whole = toggle_inline("say **hello** now", (4, 13), "**");
        assert_eq!(whole.insert, "hello");
        let word = toggle_inline("say hello now", (6, 6), "_");
        assert_eq!(word.insert, "_hello_");
        let empty = toggle_inline("a  b", (2, 2), "`");
        assert_eq!((empty.insert.as_str(), empty.cursor), ("``", 3));
    }

    #[test]
    fn links_select_what_to_type_next() {
        let e = link("see docs", (4, 8));
        assert_eq!(e.insert, "[docs](https://)");
        assert_eq!((e.anchor, e.cursor), (Some(11), 19));
        let u = link("https://x.org", (0, 13));
        assert_eq!((u.insert.as_str(), u.cursor), ("[](https://x.org)", 1));
    }

    #[test]
    fn line_tools_set_replace_and_clear() {
        assert_eq!(toggle_lines(&["a", "b"], LineTool::Number), vec!["1. a", "2. b"]);
        assert_eq!(toggle_lines(&["- a", "- b"], LineTool::Check), vec!["- [ ] a", "- [ ] b"]);
        assert_eq!(toggle_lines(&["- [ ] a"], LineTool::Check), vec!["a"]);
        assert_eq!(toggle_lines(&["## t"], LineTool::Heading(2)), vec!["t"]);
        assert_eq!(toggle_lines(&["## t"], LineTool::Heading(1)), vec!["# t"]);
        assert_eq!(toggle_lines(&["  1. x"], LineTool::Bullet), vec!["  - x"]);
        assert_eq!(toggle_lines(&["a", "", "b"], LineTool::Quote), vec!["> a", "", "> b"]);
    }

    #[test]
    fn enter_continues_and_ends_lists() {
        assert_eq!(enter("- a"), Enter::Continue("- ".into()));
        assert_eq!(enter("  3) a"), Enter::Continue("  4) ".into()));
        assert_eq!(enter("* [x] a"), Enter::Continue("* [ ] ".into()));
        assert_eq!(enter("- "), Enter::End);
        assert_eq!(enter("- [ ] "), Enter::End);
        assert_eq!(enter("plain"), Enter::Plain);
    }

    #[test]
    fn numbers_count_on() {
        let lines = ["1. a", "2. new", "2. b", "   - nested", "3. c", "", "1. other"];
        assert_eq!(renumber(&lines, 0), vec![(2, "3. b".into()), (4, "4. c".into())]);
    }

    #[test]
    fn boxes_tick_one_byte() {
        assert_eq!(toggle_box("- [ ] a"), Some((3, 'x')));
        assert_eq!(toggle_box("  - [x] a"), Some((5, ' ')));
        assert_eq!(toggle_box("- a"), None);
    }

    #[test]
    fn the_caret_lights_what_it_is_in() {
        let a = active("- [ ] a **bold** word", LineKind::Text, 11);
        assert!(a.check && a.bold && !a.italic);
        assert_eq!(active("## h", LineKind::Text, 3).heading, 2);
        assert!(active("x", LineKind::FenceBody, 0).code);
    }
}
