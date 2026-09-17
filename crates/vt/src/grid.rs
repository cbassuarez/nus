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
        assert_eq!(v, vec![Display::Line(0), Display::Fold(1, 4), Display::Line(4), Display::Line(5)]);
        // No folds: plain lines.
        assert_eq!(g.display_lines(&[]), vec![Display::Line(0), Display::Line(1), Display::Line(2), Display::Line(3)]);
        // A fold starting above the view is skipped without a row.
        g.scroll_display(-2);
        assert_eq!(g.display_lines(&[(1, 4)])[0], Display::Line(4));
    }
}
