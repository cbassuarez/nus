//! The screen: a fixed grid of cells plus a scrollback ring above it.

use std::collections::VecDeque;

use crate::cell::Cell;

#[derive(Clone, Debug)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// This row continues onto the next one because of auto-wrap (as opposed
    /// to an explicit newline). Used for copy/reflow, not for rendering.
    pub wrapped: bool,
}

impl Row {
    pub fn blank(cols: usize, template: &Cell) -> Row {
        Row {
            cells: vec![Cell::erased_from(template); cols],
            wrapped: false,
        }
    }

    pub fn text(&self) -> String {
        let s: String = self
            .cells
            .iter()
            .filter(|c| !c.flags.contains(crate::cell::Flags::WIDE_SPACER))
            .map(|c| c.ch)
            .collect();
        s.trim_end().to_string()
    }
}

#[derive(Clone, Debug)]
pub struct Grid {
    cols: usize,
    rows: usize,
    /// Visible rows, index 0 at the top.
    lines: Vec<Row>,
    /// Rows that scrolled off the top; back() is the most recent.
    scrollback: VecDeque<Row>,
    max_scrollback: usize,
    /// Per-visible-row damage since the last `take_damage`.
    damage: Vec<bool>,
    /// How many scrollback rows the viewer has scrolled up. 0 = live.
    pub display_offset: usize,
    /// The absolute index of visible row 0: rows ever pushed into history
    /// (less those pulled back). Marks are kept in absolute lines so they
    /// survive scrolling.
    history_total: u64,
}

/// Where an absolute line is right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loc {
    /// A visible row.
    Visible(usize),
    /// A scrollback row, 0 = oldest kept.
    History(usize),
}

/// One display row of a folded view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    Line(u64),
    /// A folded range `(start, end)`, end exclusive, drawn as one row.
    Fold(u64, u64),
}

impl Grid {
    pub fn new(cols: usize, rows: usize, max_scrollback: usize) -> Grid {
        let template = Cell::default();
        Grid {
            cols,
            rows,
            lines: (0..rows).map(|_| Row::blank(cols, &template)).collect(),
            scrollback: VecDeque::new(),
            max_scrollback,
            damage: vec![true; rows],
            display_offset: 0,
            history_total: 0,
        }
    }

    /// Absolute line of visible row `r`.
    pub fn abs_row(&self, r: usize) -> u64 {
        self.history_total + r as u64
    }

    /// The absolute line at the top of history still kept.
    pub fn oldest_abs(&self) -> u64 {
        self.history_total - self.scrollback.len() as u64
    }

    /// Where an absolute line is now, if it is still kept.
    pub fn locate(&self, abs: u64) -> Option<Loc> {
        if abs >= self.history_total {
            let r = (abs - self.history_total) as usize;
            (r < self.rows).then_some(Loc::Visible(r))
        } else {
            let back = (self.history_total - abs) as usize;
            (back <= self.scrollback.len()).then(|| Loc::History(self.scrollback.len() - back))
        }
    }

    /// The row at an absolute line, if kept.
    pub fn row_abs(&self, abs: u64) -> Option<&Row> {
        match self.locate(abs)? {
            Loc::Visible(r) => Some(&self.lines[r]),
            Loc::History(i) => Some(&self.scrollback[i]),
        }
    }

    /// What each display row shows once folded ranges are skipped: the
    /// absolute line, or the fold that stands in for a range. `folds` are
    /// `(start, end)` absolute lines, end exclusive, sorted; the fold's row
    /// is drawn by the host (a ruled line for the block) and the lines
    /// inside are not drawn at all. The view starts at the line
    /// `display_offset` puts at the top and runs until `rows` are filled.
    pub fn display_lines(&self, folds: &[(u64, u64)]) -> Vec<Display> {
        let rows = self.rows;
        let mut out = Vec::with_capacity(rows);
        let mut line = self.abs_of_display(0);
        let last = self.history_total + rows as u64;
        while out.len() < rows && line < last {
            match folds.iter().find(|&&(s, e)| line >= s && line < e) {
                Some(&(s, e)) => {
                    if line == s {
                        out.push(Display::Fold(s, e));
                    }
                    line = e;
                }
                None => {
                    out.push(Display::Line(line));
                    line += 1;
                }
            }
        }
        out
    }

    /// The viewer's row `r` as an absolute line, honouring `display_offset`.
    pub fn abs_of_display(&self, r: usize) -> u64 {
        let off = self.display_offset.min(self.scrollback.len()) as u64;
        self.history_total - off + r as u64
    }

    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    pub fn row(&self, r: usize) -> &Row {
        &self.lines[r]
    }

    pub fn row_mut(&mut self, r: usize) -> &mut Row {
        self.damage[r] = true;
        &mut self.lines[r]
    }

    pub fn cell(&self, r: usize, c: usize) -> &Cell {
        &self.lines[r].cells[c]
    }

    pub fn cell_mut(&mut self, r: usize, c: usize) -> &mut Cell {
        self.damage[r] = true;
        &mut self.lines[r].cells[c]
    }

    /// The row the viewer sees at visible index `r`, honouring `display_offset`.
    pub fn visible_row(&self, r: usize) -> &Row {
        if self.display_offset == 0 {
            return &self.lines[r];
        }
        let off = self.display_offset.min(self.scrollback.len());
        if r < off {
            // Rows from history: the last `off` scrollback rows, top first.
            let idx = self.scrollback.len() - off + r;
            &self.scrollback[idx]
        } else {
            &self.lines[r - off]
        }
    }

    /// Put an absolute line at the top of the view (or go live if it is
    /// on the visible screen).
    pub fn scroll_to_abs(&mut self, abs: u64) {
        let new = if abs >= self.history_total {
            0
        } else {
            ((self.history_total - abs) as usize).min(self.scrollback.len())
        };
        if new != self.display_offset {
            self.display_offset = new;
            self.damage_all();
        }
    }

    pub fn scroll_display(&mut self, delta: isize) {
        let max = self.scrollback.len() as isize;
        let new = (self.display_offset as isize + delta).clamp(0, max);
        if new as usize != self.display_offset {
            self.display_offset = new as usize;
            self.damage_all();
        }
    }

    pub fn damage_all(&mut self) {
        self.damage.iter_mut().for_each(|d| *d = true);
    }

    pub fn damage_row(&mut self, r: usize) {
        self.damage[r] = true;
    }

    /// Returns the damage flags and clears them.
    pub fn take_damage(&mut self) -> Vec<bool> {
        std::mem::replace(&mut self.damage, vec![false; self.rows])
    }

    pub fn is_damaged(&self) -> bool {
        self.damage.iter().any(|d| *d)
    }

    /// Scroll rows `top..=bottom` up by `n`: rows leave at `top`, blank rows
    /// enter at `bottom`. When the region is the whole screen and
    /// `keep_history` is set, departing rows go into scrollback.
    pub fn scroll_up(
        &mut self,
        top: usize,
        bottom: usize,
        n: usize,
        template: &Cell,
        keep_history: bool,
    ) {
        let n = n.min(bottom - top + 1);
        if n == 0 {
            return;
        }
        let full = top == 0 && bottom == self.rows - 1;
        for _ in 0..n {
            let row = self.lines.remove(top);
            if full && keep_history && self.max_scrollback > 0 {
                self.scrollback.push_back(row);
                self.history_total += 1;
                if self.scrollback.len() > self.max_scrollback {
                    self.scrollback.pop_front();
                }
            } else if full && keep_history {
                self.history_total += 1;
            }
            self.lines.insert(bottom, Row::blank(self.cols, template));
        }
        for r in top..=bottom {
            self.damage[r] = true;
        }
        // Keep the viewer anchored on the same history rows while scrolled up.
        if self.display_offset > 0 && full && keep_history {
            self.display_offset = (self.display_offset + n).min(self.scrollback.len());
        }
    }

    /// Scroll rows `top..=bottom` down by `n`: blank rows enter at `top`.
    pub fn scroll_down(&mut self, top: usize, bottom: usize, n: usize, template: &Cell) {
        let n = n.min(bottom - top + 1);
        if n == 0 {
            return;
        }
        for _ in 0..n {
            self.lines.remove(bottom);
            self.lines.insert(top, Row::blank(self.cols, template));
        }
        for r in top..=bottom {
            self.damage[r] = true;
        }
    }

    pub fn clear_all(&mut self, template: &Cell) {
        for r in 0..self.rows {
            self.lines[r] = Row::blank(self.cols, template);
        }
        self.damage_all();
    }

    pub fn clear_scrollback(&mut self) {
        self.scrollback.clear();
        self.display_offset = 0;
        self.damage_all();
    }

    /// Resize without reflow: rows are truncated or padded on the right, rows
    /// are dropped from the top (into scrollback) or added at the bottom.
    /// Returns how far the cursor row must shift to stay on the same line
    /// (negative when rows left the top, positive when history was pulled
    /// back in). When shrinking, blank rows below `cursor_row` go first; rows
    /// leave the top only when the cursor would otherwise fall off — which
    /// is what conhost/ConPTY assumes when it repaints with absolute CUPs.
    pub fn resize(
        &mut self,
        cols: usize,
        rows: usize,
        template: &Cell,
        cursor_row: usize,
    ) -> isize {
        let mut shift: isize = 0;
        let mut cursor_row = cursor_row;
        let cols = cols.max(1);
        let rows = rows.max(1);
        if cols != self.cols {
            for row in self.lines.iter_mut().chain(self.scrollback.iter_mut()) {
                row.cells.resize(cols, Cell::erased_from(template));
            }
            self.cols = cols;
        }
        while self.lines.len() > rows {
            let last = self.lines.len() - 1;
            if last > cursor_row && self.lines[last].cells.iter().all(|c| c.is_blank()) {
                self.lines.pop();
                continue;
            }
            let row = self.lines.remove(0);
            shift -= 1;
            cursor_row = cursor_row.saturating_sub(1);
            self.history_total += 1;
            if self.max_scrollback > 0 {
                self.scrollback.push_back(row);
                if self.scrollback.len() > self.max_scrollback {
                    self.scrollback.pop_front();
                }
            }
        }
        while self.lines.len() < rows {
            // Pull rows back out of history first, so shrinking then growing
            // a window doesn't lose what was on screen.
            match self.scrollback.pop_back() {
                Some(row) if self.max_scrollback > 0 => {
                    self.lines.insert(0, row);
                    self.history_total -= 1;
                    shift += 1;
                }
                _ => self.lines.push(Row::blank(cols, template)),
            }
        }
        self.rows = rows;
        self.damage = vec![true; rows];
        self.display_offset = self.display_offset.min(self.scrollback.len());
        shift
    }

    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|r| r.text())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// What to look for in a terminal's text: plain characters (compared
/// case-insensitively unless `case`), or a pattern. Whole-word plain text
/// and regular expressions go through the regex crate, which runs in time
/// linear in the text: no pattern can hang a search.
#[derive(Clone, Debug)]
pub struct Needle {
    chars: Vec<char>,
    case: bool,
    pattern: Option<regex::Regex>,
}

impl Needle {
    /// Plain text.
    pub fn new(text: &str, case: bool) -> Needle {
        Needle {
            chars: text.chars().map(|c| fold(c, case)).collect(),
            case,
            pattern: None,
        }
    }

    /// Plain text or a pattern, whole words or not. The error says what is
    /// wrong with a pattern, in a line.
    pub fn with(text: &str, case: bool, word: bool, regex: bool) -> Result<Needle, String> {
        if !word && !regex {
            return Ok(Needle::new(text, case));
        }
        let mut source = if regex {
            text.to_string()
        } else {
            regex::escape(text)
        };
        if word {
            // A boundary only where the text has a word character to bound.
            let wordy = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            let (head, tail) = if regex {
                (true, true)
            } else {
                (wordy(text.chars().next()), wordy(text.chars().last()))
            };
            source = format!(
                "{}(?:{source}){}",
                if head { r"\b" } else { "" },
                if tail { r"\b" } else { "" }
            );
        }
        let pattern = regex::RegexBuilder::new(&source)
            .case_insensitive(!case)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| match e {
                regex::Error::Syntax(s) => s
                    .lines()
                    .last()
                    .unwrap_or("not a pattern")
                    .trim()
                    .trim_start_matches("error: ")
                    .to_string(),
                regex::Error::CompiledTooBig(_) => "pattern too large".into(),
                _ => "not a pattern".into(),
            })?;
        Ok(Needle {
            chars: text.chars().collect(),
            case,
            pattern: Some(pattern),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }
}

fn fold(c: char, case: bool) -> char {
    if case {
        c
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// A match in the terminal's text, in cells: from (`line`, `col`) to
/// (`end_line`, `end_col`), end exclusive. A match in a line the terminal
/// wrapped can start on one row and end on the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Found {
    pub line: u64,
    pub col: usize,
    pub end_line: u64,
    pub end_col: usize,
}

impl Found {
    /// The cells of this match on row `line`: (first col, cols), if any.
    pub fn on_row(&self, line: u64, cols: usize) -> Option<(usize, usize)> {
        if line < self.line || line > self.end_line {
            return None;
        }
        let from = if line == self.line { self.col } else { 0 };
        let to = if line == self.end_line {
            self.end_col
        } else {
            cols
        };
        (to > from).then_some((from, to - from))
    }
}

impl Grid {
    /// One past the last line there is.
    pub fn end_abs(&self) -> u64 {
        self.history_total + self.rows as u64
    }

    /// The first row of the logical line `abs` is part of: the rows the
    /// terminal joined by wrapping, as opposed to ones a newline ended.
    pub fn logical_start(&self, abs: u64) -> u64 {
        let oldest = self.oldest_abs();
        let mut l = abs.max(oldest);
        while l > oldest {
            match self.row_abs(l - 1) {
                Some(r) if r.wrapped => l -= 1,
                _ => break,
            }
        }
        l
    }

    /// Matches of `needle` in the logical lines that start in
    /// [`from`, `to`), each searched to its end even past `to`. `from`
    /// should be a logical start. Returns the matches in order and the
    /// line after the last logical line searched (the next `from`).
    /// Matches do not overlap; wide characters count once, and their
    /// spacer cells are not text.
    pub fn find_in(&self, needle: &Needle, from: u64, to: u64) -> (Vec<Found>, u64) {
        let mut out = Vec::new();
        let end = self.end_abs();
        let mut l = from.max(self.oldest_abs());
        if needle.is_empty() {
            return (out, to.min(end).max(l));
        }
        let n = needle.chars.len();
        // (char, line, col, width) for each character of the logical line.
        let mut text: Vec<(char, u64, usize, usize)> = Vec::new();
        while l < to && l < end {
            text.clear();
            let mut row_line = l;
            while let Some(row) = self.row_abs(row_line) {
                for (col, c) in row.cells.iter().enumerate() {
                    if c.flags.contains(crate::cell::Flags::WIDE_SPACER) {
                        continue;
                    }
                    let w = if c.flags.contains(crate::cell::Flags::WIDE) {
                        2
                    } else {
                        1
                    };
                    let ch = if needle.pattern.is_some() {
                        c.ch
                    } else {
                        fold(c.ch, needle.case)
                    };
                    text.push((ch, row_line, col, w));
                }
                if !row.wrapped || row_line + 1 >= end {
                    break;
                }
                row_line += 1;
            }
            if let Some(re) = &needle.pattern {
                // The logical line as a string, each char's byte offset
                // mapped back to its cell.
                let s: String = text.iter().map(|t| t.0).collect();
                let mut at = Vec::with_capacity(text.len() + 1);
                for (k, (b, _)) in s.char_indices().enumerate() {
                    at.push((b, k));
                }
                let index = |b: usize| {
                    at.binary_search_by_key(&b, |p| p.0)
                        .map(|i| at[i].1)
                        .unwrap_or(text.len())
                };
                for m in re.find_iter(&s) {
                    if m.start() == m.end() {
                        continue;
                    }
                    let (a, z) = (index(m.start()), index(m.end()).saturating_sub(1));
                    let (first, last) = (text[a], text[z]);
                    out.push(Found {
                        line: first.1,
                        col: first.2,
                        end_line: last.1,
                        end_col: last.2 + last.3,
                    });
                }
                l = row_line + 1;
                continue;
            }
            let mut i = 0;
            while i + n <= text.len() {
                if text[i..i + n]
                    .iter()
                    .zip(&needle.chars)
                    .all(|(t, c)| t.0 == *c)
                {
                    let first = text[i];
                    let last = text[i + n - 1];
                    out.push(Found {
                        line: first.1,
                        col: first.2,
                        end_line: last.1,
                        end_col: last.2 + last.3,
                    });
                    i += n;
                } else {
                    i += 1;
                }
            }
            l = row_line + 1;
        }
        (out, l)
    }
}

#[cfg(test)]
mod fold_tests {
    use super::*;

    #[test]
    fn folds_collapse_ranges_into_one_row() {
        let mut g = Grid::new(4, 4, 100);
        // Push six lines into history so abs lines 0..6 exist, 6..10 visible.
        for _ in 0..6 {
            g.scroll_up(0, 3, 1, &Cell::default(), true);
        }
        assert_eq!(g.abs_row(0), 6);
        g.scroll_display(6);
        // View starts at abs 0; fold 1..4 → rows: 0, F(1,4), 4, 5.
        let v = g.display_lines(&[(1, 4)]);
        assert_eq!(
            v,
            vec![
                Display::Line(0),
                Display::Fold(1, 4),
                Display::Line(4),
                Display::Line(5)
            ]
        );
        // No folds: plain lines.
        assert_eq!(
            g.display_lines(&[]),
            vec![
                Display::Line(0),
                Display::Line(1),
                Display::Line(2),
                Display::Line(3)
            ]
        );
        // A fold starting above the view is skipped without a row.
        g.scroll_display(-2);
        assert_eq!(g.display_lines(&[(1, 4)])[0], Display::Line(4));
    }
}

#[cfg(test)]
mod find_tests {
    use super::*;
    use crate::cell::Flags;

    fn grid(rows: &[(&str, bool)]) -> Grid {
        let cols = 8;
        let mut g = Grid::new(cols, rows.len(), 0);
        for (r, (text, wrapped)) in rows.iter().enumerate() {
            let mut cells = Vec::new();
            for ch in text.chars() {
                let wide = matches!(ch, '漢' | '字');
                cells.push(Cell {
                    ch,
                    flags: if wide { Flags::WIDE } else { Flags::empty() },
                    ..Cell::default()
                });
                if wide {
                    cells.push(Cell {
                        ch: ' ',
                        flags: Flags::WIDE_SPACER,
                        ..Cell::default()
                    });
                }
            }
            cells.resize(cols, Cell::default());
            g.lines[r] = Row {
                cells,
                wrapped: *wrapped,
            };
        }
        g
    }

    #[test]
    fn a_word_split_by_a_wrap_is_one_match_on_two_rows() {
        let g = grid(&[("xlast_ER", true), ("ROR=1", false), ("ok", false)]);
        let (hits, next) = g.find_in(&Needle::new("error", false), 0, 3);
        assert_eq!(
            hits,
            vec![Found {
                line: 0,
                col: 6,
                end_line: 1,
                end_col: 3
            }]
        );
        assert_eq!(next, 3);
        assert_eq!(hits[0].on_row(0, 8), Some((6, 2)));
        assert_eq!(hits[0].on_row(1, 8), Some((0, 3)));
        assert_eq!(hits[0].on_row(2, 8), None);
        assert_eq!(g.logical_start(1), 0);
        assert_eq!(g.logical_start(2), 2);
    }

    #[test]
    fn case_and_wide_characters() {
        let g = grid(&[("a漢字b", false), ("Error", false)]);
        let (hits, _) = g.find_in(&Needle::new("字b", false), 0, 2);
        // 漢 is cells 1-2, 字 3-4, b at 5.
        assert_eq!(
            hits,
            vec![Found {
                line: 0,
                col: 3,
                end_line: 0,
                end_col: 6
            }]
        );
        assert_eq!(g.find_in(&Needle::new("error", true), 0, 2).0.len(), 0);
        assert_eq!(g.find_in(&Needle::new("Error", true), 0, 2).0.len(), 1);
    }

    #[test]
    fn matches_do_not_overlap_and_a_range_ends_on_a_logical_line() {
        let g = grid(&[("aaaa", true), ("aa", false), ("aa", false)]);
        let (hits, next) = g.find_in(&Needle::new("aa", false), 0, 1);
        // The first logical line runs on to row 1 even though `to` is 1.
        assert_eq!(hits.len(), 3);
        assert_eq!(next, 2);
        assert!(g.find_in(&Needle::new("", false), 0, 3).0.is_empty());
    }

    #[test]
    fn words_and_patterns() {
        let g = grid(&[("err errs", false), ("ERR:5 x", false)]);
        let word = Needle::with("err", false, true, false).unwrap();
        let hits = g.find_in(&word, 0, 2).0;
        assert_eq!(
            hits.iter().map(|h| (h.line, h.col)).collect::<Vec<_>>(),
            vec![(0, 0), (1, 0)]
        );
        let re = Needle::with(r"err\w+", true, false, true).unwrap();
        assert_eq!(
            g.find_in(&re, 0, 2).0,
            vec![Found {
                line: 0,
                col: 4,
                end_line: 0,
                end_col: 8
            }]
        );
        // Across a wrap, and on wide characters.
        let w = grid(&[("xlast_ER", true), ("ROR=1", false), ("a漢字b", false)]);
        let re = Needle::with("ER+OR", true, false, true).unwrap();
        assert_eq!(
            w.find_in(&re, 0, 3).0,
            vec![Found {
                line: 0,
                col: 6,
                end_line: 1,
                end_col: 3
            }]
        );
        let re = Needle::with("字.", false, false, true).unwrap();
        assert_eq!(
            w.find_in(&re, 0, 3).0,
            vec![Found {
                line: 2,
                col: 3,
                end_line: 2,
                end_col: 6
            }]
        );
        // Patterns that match nothing visible are skipped; bad ones say why.
        assert!(w
            .find_in(&Needle::with("x*", false, false, true).unwrap(), 0, 3)
            .0
            .iter()
            .all(|h| h.end_col > h.col || h.end_line > h.line));
        assert!(Needle::with("(fail|err", false, false, true)
            .unwrap_err()
            .contains("unclosed"));
    }
}
