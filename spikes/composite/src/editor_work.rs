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
