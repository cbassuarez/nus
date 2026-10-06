//! A note's lines as they are drawn: wrapped to the column, each row with
//! where every character of it starts. The note's face may be
//! proportional (Areal is), so nothing here assumes a cell: drawing, the
//! caret, the selection, a click and Up/Down all read the same stops.
//! Built each frame for the rows on screen (editor.rs, `draw_note_body`).

/// One row on screen: columns `start..end` of a line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    /// The row's top, its height (a heading's row is taller, its first
    /// with room above), its baseline and the size its words are set at.
    pub y: f32,
    pub h: f32,
    pub base: f32,
    pub px: f32,
    /// Where its first character starts.
    pub x0: f32,
    /// Stops from `x0`, one per column `start..=end`.
    pub xs: Vec<f32>,
    /// The line's last row (a caret at `end` stays here, not on the next).
    pub last: bool,
}

impl Row {
    pub fn x(&self, col: usize) -> f32 {
        let k = col.clamp(self.start, self.end) - self.start;
        self.x0 + self.xs.get(k).copied().unwrap_or(0.0)
    }

    /// The column nearest `x`. On a row the line goes on from, the last
    /// stop belongs to the next row, so the caret never lands there.
    fn nearest(&self, x: f32) -> usize {
        let top = if self.last { self.xs.len() } else { self.xs.len().saturating_sub(1) }.max(1);
        let rel = x - self.x0;
        let k = (0..top).min_by(|&a, &b| (self.xs[a] - rel).abs().total_cmp(&(self.xs[b] - rel).abs())).unwrap_or(0);
        self.start + k
    }
}

/// The rows on screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub rows: Vec<Row>,
}

impl Layout {
    /// The row a caret at (line, col) stands on: where a line wraps, the
    /// column at the break starts the next row.
    pub fn row_of(&self, line: usize, col: usize) -> Option<usize> {
        let mut near = None;
        for (i, r) in self.rows.iter().enumerate().filter(|(_, r)| r.line == line) {
            if col >= r.start && (col < r.end || r.last || r.start == r.end) {
                return Some(i);
            }
            if col >= r.start {
                near = Some(i);
            }
        }
        near
    }

    /// Where a caret at (line, col) is drawn: its x and its row's top.
    pub fn caret(&self, line: usize, col: usize) -> Option<(f32, f32)> {
        let r = &self.rows[self.row_of(line, col)?];
        Some((r.x(col), r.y))
    }

    /// The (line, col) nearest a point: above the rows is the first,
    /// below them the last.
    pub fn hit(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let i = self.rows.iter().position(|r| y < r.y + r.h).unwrap_or(self.rows.len().checked_sub(1)?);
        let r = &self.rows[i];
        Some((r.line, r.nearest(x)))
    }

    /// Up or Down from (line, col), keeping to `x`: None when the row it
    /// would go to is not on screen.
    pub fn step(&self, line: usize, col: usize, x: f32, down: bool) -> Option<(usize, usize)> {
        let i = self.row_of(line, col)?;
        let j = if down { i + 1 } else { i.checked_sub(1)? };
        let r = self.rows.get(j)?;
        Some((r.line, r.nearest(x)))
    }
}

/// Where a line breaks, given each character's width: rows of columns
/// `start..end`. A row breaks after the last space that fits; a word
/// longer than the row breaks where it must. Spaces at a row's end hang
/// past the edge. Rows after the first are `hang` narrower (a list
/// item's words line up under its first word).
pub fn wrap(chars: &[char], widths: &[f32], width: f32, hang: f32) -> Vec<(usize, usize)> {
    let n = chars.len().min(widths.len());
    let mut rows = Vec::new();
    let (mut start, mut x, mut brk) = (0usize, 0.0f32, None::<usize>);
    for i in 0..n {
        let w = widths[i];
        if chars[i].is_whitespace() {
            x += w;
            brk = Some(i + 1);
            continue;
        }
        let avail = if rows.is_empty() { width } else { width - hang };
        if x + w > avail && i > start {
            let at = match brk {
                Some(b) if b > start => b,
                _ => i,
            };
            rows.push((start, at));
            start = at;
            x = widths[start..i].iter().sum();
            brk = None;
        }
        x += w;
    }
    rows.push((start, n));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(s: &str) -> (Vec<char>, Vec<f32>) {
        let c: Vec<char> = s.chars().collect();
        let w = vec![1.0; c.len()];
        (c, w)
    }

    #[test]
    fn breaks_after_the_last_space_that_fits() {
        let (c, w) = widths("aaa bbb ccc");
        assert_eq!(wrap(&c, &w, 8.0, 0.0), vec![(0, 8), (8, 11)]);
    }

    #[test]
    fn a_word_too_long_breaks_where_it_must() {
        let (c, w) = widths("abcdefghij");
        assert_eq!(wrap(&c, &w, 4.0, 0.0), vec![(0, 4), (4, 8), (8, 10)]);
    }

    #[test]
    fn an_empty_line_is_one_empty_row() {
        assert_eq!(wrap(&[], &[], 10.0, 0.0), vec![(0, 0)]);
    }

    #[test]
    fn spaces_hang_past_the_edge() {
        let (c, w) = widths("aaaa    bb");
        assert_eq!(wrap(&c, &w, 4.0, 0.0), vec![(0, 8), (8, 10)]);
    }

    #[test]
    fn rows_after_the_first_are_narrower_by_the_hang() {
        let (c, w) = widths("- aa bb cc dd");
        assert_eq!(wrap(&c, &w, 6.0, 2.0), vec![(0, 5), (5, 8), (8, 11), (11, 13)]);
    }

    fn layout() -> Layout {
        // "aaa bbb" wrapped after the space, then a second line "cc".
        let row = |line, start, end, y: f32, last| Row { line, start, end, y, h: 10.0, base: y + 8.0, px: 10.0, x0: 10.0, xs: (0..=end - start).map(|k| k as f32 * 2.0).collect(), last };
        Layout { rows: vec![row(0, 0, 4, 0.0, false), row(0, 4, 7, 10.0, true), row(1, 0, 2, 20.0, true)] }
    }

    #[test]
    fn a_caret_at_a_wrap_starts_the_next_row() {
        let l = layout();
        assert_eq!(l.row_of(0, 4), Some(1));
        assert_eq!(l.caret(0, 4), Some((10.0, 10.0)));
        assert_eq!(l.row_of(0, 7), Some(1));
        assert_eq!(l.caret(1, 2), Some((14.0, 20.0)));
    }

    #[test]
    fn a_click_finds_the_nearest_column_on_its_row() {
        let l = layout();
        assert_eq!(l.hit(13.2, 3.0), Some((0, 2)));
        // Past the end of a wrapped row: its last character, not the break.
        assert_eq!(l.hit(100.0, 3.0), Some((0, 3)));
        assert_eq!(l.hit(100.0, 13.0), Some((0, 7)));
        assert_eq!(l.hit(0.0, 500.0), Some((1, 0)));
    }

    #[test]
    fn a_tall_row_is_hit_over_its_whole_height() {
        let mut l = layout();
        // The first row is a heading's: twice as tall, the rest move down.
        l.rows[0].h = 20.0;
        l.rows[1].y = 20.0;
        l.rows[2].y = 30.0;
        assert_eq!(l.hit(10.0, 15.0).map(|h| h.0), Some(0));
        assert_eq!(l.hit(14.0, 25.0), Some((0, 6)));
    }

    #[test]
    fn up_and_down_move_by_rows() {
        let l = layout();
        assert_eq!(l.step(0, 1, 12.0, true), Some((0, 5)));
        assert_eq!(l.step(0, 5, 12.0, true), Some((1, 1)));
        assert_eq!(l.step(1, 1, 12.0, false), Some((0, 5)));
        assert_eq!(l.step(0, 1, 12.0, false), None);
    }
}
