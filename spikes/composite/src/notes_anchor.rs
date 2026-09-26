//! Finding a captured excerpt in its source again. A file capture keeps
//! the lines it came from and their exact text; the file has likely moved
//! on since. Where the text is still at those lines, that is the place;
//! where it is somewhere else, once, that is the place and it moved; where
//! it is in several places, you choose; where it is nowhere, the note's
//! copy is what there is. A click never jumps confidently to a guess.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// At the captured line (0-based), unchanged.
    Found(usize),
    /// Once, elsewhere: the file changed around it.
    Moved(usize),
    /// In several places: these lines.
    Ambiguous(Vec<usize>),
    /// Not in the file any more.
    Missing,
}

/// Where `quote` is in `text`, given the line it was captured at.
pub fn locate(text: &str, quote: &str, hint: usize) -> Anchor {
    let text = text.replace("\r\n", "\n");
    let quote = quote.replace("\r\n", "\n");
    let quote = quote.trim_end_matches('\n');
    if quote.trim().is_empty() {
        return Anchor::Missing;
    }
    let mut lines = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find(quote) {
        let at = from + i;
        lines.push(text[..at].matches('\n').count());
        from = at + quote.len().max(1);
        if lines.len() > 64 {
            break;
        }
    }
    match lines.len() {
        0 => Anchor::Missing,
        1 if lines[0] == hint => Anchor::Found(hint),
        1 => Anchor::Moved(lines[0]),
        _ if lines.contains(&hint) => Anchor::Found(hint),
        _ => Anchor::Ambiguous(lines),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_moved_ambiguous_missing() {
        let text = "a\nfn keep() {}\nb\n";
        assert_eq!(locate(text, "fn keep() {}", 1), Anchor::Found(1));
        assert_eq!(locate(&format!("x\ny\n{text}"), "fn keep() {}", 1), Anchor::Moved(3));
        assert_eq!(locate("x\nr\nx\n", "x", 1), Anchor::Ambiguous(vec![0, 2]));
        assert_eq!(locate("x\nr\nx\n", "x", 2), Anchor::Found(2), "the captured place, when it is one of them");
        assert_eq!(locate(text, "gone", 1), Anchor::Missing);
        assert_eq!(locate(text, "  ", 1), Anchor::Missing);
    }

    #[test]
    fn multiline_and_crlf() {
        let text = "one\r\ntwo\r\nthree\r\n";
        assert_eq!(locate(text, "two\nthree\n", 1), Anchor::Found(1));
    }
}
