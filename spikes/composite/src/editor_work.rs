//! File-size-independent editor operations and work for the bounded pool.
use crate::predict::Tok;
use nus_lsp::lsp_types::Position;
use ropey::Rope;
use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read},
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

pub const MAX_MATCHES: usize = 10_000;
pub type LineSpans = HashMap<usize, Vec<(usize, usize, Tok)>>;

pub fn load(path: &Path, cancel: &AtomicUsize) -> io::Result<Rope> {
    if crate::protected_state::is_private_path(path){return crate::protected_state::read_text(path).map(|text|Rope::from_str(&text));}
    struct Reader<'a>(std::fs::File, &'a AtomicUsize);
    impl Read for Reader<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.1.load(Ordering::Relaxed) != 0 {
                return Err(io::Error::new(io::ErrorKind::Other, "file closed"));
            }
            self.0.read(out)
        }
    }
    // Build the rope directly. No second full-file String or CRLF copy.
    Rope::from_reader(Reader(std::fs::File::open(path)?, cancel))
}

pub fn offset(text: &Rope, pos: Position) -> usize {
    let line = pos.line as usize;
    if line >= text.len_lines() {
        return text.len_chars();
    }
    let start = text.line_to_char(line);
    let line = text.line(line);
    let units = (pos.character as usize).min(line.len_utf16_cu());
    start + line.utf16_cu_to_char(units)
}

pub fn position(text: &Rope, at: usize) -> Position {
    let at = at.min(text.len_chars());
    let line = text.char_to_line(at);
    let start = text.line_to_char(line);
    Position::new(
        line as u32,
        text.line(line).char_to_utf16_cu(at - start) as u32,
    )
}

pub fn highlight(
    text: &Rope,
    grammar: &str,
    start: usize,
    end: usize,
    cancel: &AtomicUsize,
) -> LineSpans {
    let source = text.slice(start..end).to_string();
    let mut per = HashMap::new();
    for (a, len, class) in
        crate::syntax::spans_cancellable(grammar, &source, Some(cancel)).unwrap_or_default()
    {
        let (a, end) = (start + a, start + a + len);
        let l0 = text.char_to_line(a);
        let l1 = text.char_to_line(end);
        for line in l0..=l1 {
            let ls = text.line_to_char(line);
            let le = ls + text.line(line).len_chars();
            let (s, e) = (a.max(ls), end.min(le));
            if e > s {
                per.entry(line)
                    .or_insert_with(Vec::new)
                    .push((s - ls, e - s, class));
            }
        }
    }
    per
}

#[derive(Default)]
pub struct Matches {
    pub ranges: Vec<(usize, usize)>,
    pub truncated: bool,
}

/// How find matches: case, whole words, a regular expression.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Opts {
    pub case: bool,
    pub word: bool,
    pub regex: bool,
}

/// A whole-word or regex search, compiled; the error says what is wrong
/// with a pattern. None for plain text (the streaming search does that).
pub fn pattern(query: &str, o: Opts) -> Result<Option<regex::Regex>, String> {
    if !o.word && !o.regex {
        return Ok(None);
    }
    let mut source = if o.regex { query.to_string() } else { regex::escape(query) };
    if o.word {
        let wordy = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        let (head, tail) = if o.regex { (true, true) } else { (wordy(query.chars().next()), wordy(query.chars().last())) };
        source = format!("{}(?:{source}){}", if head { r"\b" } else { "" }, if tail { r"\b" } else { "" });
    }
    regex::RegexBuilder::new(&source)
        .case_insensitive(!o.case)
        .size_limit(1 << 20)
        .build()
        .map(Some)
        .map_err(|e| match e {
            regex::Error::Syntax(s) => s.lines().last().unwrap_or("not a pattern").trim().trim_start_matches("error: ").to_string(),
            regex::Error::CompiledTooBig(_) => "pattern too large".into(),
            _ => "not a pattern".into(),
        })
}

/// Find with options. A pattern runs line by line (a match never spans a
/// line break, as in most editors' find); plain text streams.
pub fn search_with(text: &Rope, query: &str, o: Opts, re: Option<&regex::Regex>, cancel: &AtomicUsize) -> Matches {
    let Some(re) = re else {
        return if o.case { search_case(text, query, cancel) } else { search(text, query, cancel) };
    };
    let mut out = Matches::default();
    let mut start = 0usize;
    for (n, line) in text.lines().enumerate() {
        if n % 256 == 0 && cancel.load(Ordering::Relaxed) != 0 {
            return Matches::default();
        }
        let s = line.to_string();
        let mut chars = s.char_indices().map(|(b, _)| b).collect::<Vec<_>>();
        chars.push(s.len());
        for m in re.find_iter(&s) {
            if m.start() == m.end() {
                continue;
            }
            if out.ranges.len() == MAX_MATCHES {
                out.truncated = true;
                return out;
            }
            let a = chars.partition_point(|&b| b < m.start());
            let z = chars.partition_point(|&b| b < m.end());
            out.ranges.push((start + a, start + z));
        }
        start += line.len_chars();
    }
    out
}

/// Case-sensitive plain text: the same walk without folding.
fn search_case(text: &Rope, query: &str, cancel: &AtomicUsize) -> Matches {
    let q: Vec<char> = query.chars().collect();
    let mut out = Matches::default();
    if q.is_empty() {
        return out;
    }
    let mut window: VecDeque<char> = VecDeque::with_capacity(q.len());
    for (at, c) in text.chars().enumerate() {
        if at % 4096 == 0 && cancel.load(Ordering::Relaxed) != 0 {
            return Matches::default();
        }
        if window.len() == q.len() {
            window.pop_front();
        }
        window.push_back(c);
        if window.len() == q.len() && window.iter().eq(q.iter()) {
            let start = at + 1 - q.len();
            if out.ranges.last().is_none_or(|&(_, end)| start >= end) {
                if out.ranges.len() == MAX_MATCHES {
                    out.truncated = true;
                    return out;
                }
                out.ranges.push((start, at + 1));
            }
        }
    }
    out
}

/// Streaming KMP: O(file + query), with original char positions preserved
/// even when Unicode lowercase expands one character into several.
pub fn search(text: &Rope, query: &str, cancel: &AtomicUsize) -> Matches {
    let q: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let mut out = Matches::default();
    if q.is_empty() {
        return out;
    }
    let mut prefix = vec![0; q.len()];
    for i in 1..q.len() {
        let mut j = prefix[i - 1];
        while j > 0 && q[i] != q[j] {
            j = prefix[j - 1];
        }
        if q[i] == q[j] {
            j += 1;
        }
        prefix[i] = j;
    }
    let mut origins = VecDeque::with_capacity(q.len());
    let mut j = 0;
    for (at, c) in text.chars().enumerate() {
        if at % 4096 == 0 && cancel.load(Ordering::Relaxed) != 0 {
            return Matches::default();
        }
        for c in c.to_lowercase() {
            if origins.len() == q.len() {
                origins.pop_front();
            }
            origins.push_back(at);
            while j > 0 && c != q[j] {
                j = prefix[j - 1];
            }
            if c == q[j] {
                j += 1;
            }
            if j == q.len() {
                if out.ranges.len() == MAX_MATCHES {
                    out.truncated = true;
                    return out;
                }
                let start = *origins.front().unwrap();
                if out.ranges.last().is_none_or(|&(_, end)| start >= end) {
                    out.ranges.push((start, at + 1));
                }
                j = 0;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn options_case_words_and_patterns() {
        let text = Rope::from_str("Err err errno\nline ERR 漢字x");
        let c = AtomicUsize::new(0);
        let plain = |q: &str, o: Opts| { let re = pattern(q, o).unwrap(); search_with(&text, q, o, re.as_ref(), &c).ranges };
        assert_eq!(plain("err", Opts::default()).len(), 4);
        assert_eq!(plain("err", Opts { case: true, ..Opts::default() }), vec![(4, 7), (8, 11)]);
        assert_eq!(plain("err", Opts { word: true, ..Opts::default() }), vec![(0, 3), (4, 7), (19, 22)]);
        assert_eq!(plain("字.", Opts { regex: true, ..Opts::default() }), vec![(24, 26)]);
        assert!(pattern("(oops", Opts { regex: true, ..Opts::default() }).unwrap_err().contains("unclosed"));
    }
    #[test]
    fn unicode_positions_and_search_use_original_char_offsets() {
        let text = Rope::from_str("😀İx\r\nlast 😀 line");
        for i in 0..=text.len_chars() {
            assert_eq!(offset(&text, position(&text, i)), i);
        }
        assert_eq!(
            search(&text, "x", &AtomicUsize::new(0)).ranges,
            vec![(2, 3)]
        );
        assert_eq!(
            search(&text, "i\u{307}x", &AtomicUsize::new(0)).ranges,
            vec![(1, 3)]
        );
    }
    #[test]
    fn search_is_bounded_cancellable_and_crosses_rope_chunks() {
        let text = Rope::from_str(&format!(
            "{}needle{}",
            " ".repeat(1023),
            " a".repeat(20_000)
        ));
        assert_eq!(
            search(&text, "needle", &AtomicUsize::new(0)).ranges,
            vec![(1023, 1029)]
        );
        let m = search(&text, "a", &AtomicUsize::new(0));
        assert_eq!(m.ranges.len(), MAX_MATCHES);
        assert!(m.truncated);
        assert!(search(&text, "a", &AtomicUsize::new(1)).ranges.is_empty());
    }
    #[test]
    fn load_keeps_crlf_and_rejects_invalid_utf8() {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all("α\r\nb\n".as_bytes()).unwrap();
        assert_eq!(
            load(file.path(), &AtomicUsize::new(0)).unwrap().to_string(),
            "α\r\nb\n"
        );
        file.write_all(&[255]).unwrap();
        assert!(load(file.path(), &AtomicUsize::new(0)).is_err());
    }

    #[test]
    #[ignore = "release timing probe; not a CI speed assertion"]
    fn release_measurements() {
        use std::{io::Write, time::Instant};
        let cancel = AtomicUsize::new(0);
        let mut results = serde_json::Map::new();
        let summarize = |mut times: Vec<f64>| {
            times.sort_by(f64::total_cmp);
            serde_json::json!({"samples":times.len(),"p50_ms":times[times.len()/2],"p95_ms":times[(times.len()*95).div_ceil(100)-1],"max_ms":times.last()})
        };
        for mib in [10, 100] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            let chunk = b"let value = 123; // a representative source line\n".repeat(1024);
            let bytes = mib * 1024 * 1024;
            let mut left = bytes;
            while left > 0 {
                let n = left.min(chunk.len());
                file.write_all(&chunk[..n]).unwrap();
                left -= n;
            }
            file.flush().unwrap();
            let mut timings = Vec::new();
            for _ in 0..7 {
                let at = Instant::now();
                let text = load(file.path(), &cancel).unwrap();
                timings.push(at.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(text.len_bytes(), bytes);
            }
            results.insert(format!("load_{mib}m_warm_cache"), summarize(timings));
        }
        let text = Rope::from_str(&"let value = 123; // source\n".repeat(500_000));
        let _ = highlight(&text, "rust", 0, 16384, &cancel); // grammar/query warmup
        let mut times = Vec::new();
        for _ in 0..100 {
            let at = Instant::now();
            let _ = highlight(&text, "rust", 0, 16384, &cancel);
            times.push(at.elapsed().as_secs_f64() * 1000.0);
        }
        results.insert("syntax_16k_rust_warm".into(), summarize(times));
        let mut times = Vec::new();
        for _ in 0..100 {
            let at = Instant::now();
            for i in 0..100 {
                let _ = offset(&text, Position::new(400_000 + i, 12));
            }
            times.push(at.elapsed().as_secs_f64() * 1000.0);
        }
        results.insert("100_diagnostic_positions".into(), summarize(times));
        eprintln!("EDITOR_BENCH {}", serde_json::Value::Object(results));
    }
}
