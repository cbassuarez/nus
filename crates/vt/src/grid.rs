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
        }
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
                if self.scrollback.len() > self.max_scrollback {
                    self.scrollback.pop_front();
                }
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
    /// back in).
    pub fn resize(&mut self, cols: usize, rows: usize, template: &Cell) -> isize {
        let mut shift: isize = 0;
        let cols = cols.max(1);
        let rows = rows.max(1);
        if cols != self.cols {
            for row in self.lines.iter_mut().chain(self.scrollback.iter_mut()) {
                row.cells.resize(cols, Cell::erased_from(template));
            }
            self.cols = cols;
        }
        while self.lines.len() > rows {
            let row = self.lines.remove(0);
            shift -= 1;
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
