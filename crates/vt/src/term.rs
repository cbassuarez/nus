//! Terminal state: screens, cursor, modes, and the `vte::ansi::Handler`
//! implementation that drives them.

use bitflags::bitflags;
use unicode_width::UnicodeWidthChar;
use vte::ansi::{
    self, Attr, CharsetIndex, ClearMode, CursorShape, CursorStyle, Handler, Hyperlink,
    KeyboardModes, KeyboardModesApplyBehavior, LineClearMode, Mode, ModifyOtherKeys, NamedColor,
    NamedMode, NamedPrivateMode, PrivateMode, Processor, StandardCharset, TabulationClearMode,
};

use crate::cell::{Cell, Color, Flags};
use crate::grid::Grid;
use crate::palette::{Palette, Rgb};

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Modes: u32 {
        const INSERT            = 1 << 0;
        const LINEFEED_NEWLINE  = 1 << 1;
        const APP_CURSOR        = 1 << 2;
        const ORIGIN            = 1 << 3;
        const AUTOWRAP          = 1 << 4;
        const SHOW_CURSOR       = 1 << 5;
        const BLINK_CURSOR      = 1 << 6;
        const BRACKETED_PASTE   = 1 << 7;
        const MOUSE_CLICK       = 1 << 8;
        const MOUSE_MOTION      = 1 << 9;
        const MOUSE_ANY         = 1 << 10;
        const MOUSE_SGR         = 1 << 11;
        const MOUSE_UTF8        = 1 << 12;
        const FOCUS_EVENTS      = 1 << 13;
        const ALT_SCREEN        = 1 << 14;
        const ALTERNATE_SCROLL  = 1 << 15;
        const APP_KEYPAD        = 1 << 16;

        const ANY_MOUSE = Self::MOUSE_CLICK.bits() | Self::MOUSE_MOTION.bits() | Self::MOUSE_ANY.bits();
    }
}

/// Things the host needs to act on; drained with [`Term::take_events`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Bell,
    Title(String),
    /// A shell-integration mark arrived (OSC 133).
    Mark(MarkKind),
    /// The shell reported its working directory (OSC 7).
    Cwd(String),
    /// Progress from the shell (OSC 9;4): state, percent.
    Progress(u8, u8),
    ClipboardStore(u8, Vec<u8>),
    ClipboardLoad(u8),
    CursorStyle(CursorStyle),
    /// The palette or default colors changed; renderer caches are stale.
    ColorsChanged,
}

/// Shell integration marks (OSC 133), in the order a command goes through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    /// A: the prompt begins.
    PromptStart,
    /// B: the prompt ends; the command line begins.
    CommandStart,
    /// C: the command runs; output follows.
    OutputStart,
    /// D: the command finished, with its exit code when the shell knows it.
    CommandEnd(Option<i32>),
}

/// A mark at an absolute line and column of the primary screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub line: u64,
    pub col: usize,
    pub kind: MarkKind,
}

#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// Attributes new cells are stamped with.
    pub template: Cell,
    /// Cursor sits past the last column; the next print wraps first.
    pub wrap_next: bool,
    pub charset: CharsetIndex,
    pub charsets: [StandardCharset; 4],
}

impl Default for Cursor {
    fn default() -> Self {
        Cursor {
            row: 0,
            col: 0,
            template: Cell::default(),
            wrap_next: false,
            charset: CharsetIndex::G0,
            charsets: [StandardCharset::Ascii; 4],
        }
    }
}

pub struct Term {
    processor: Processor,
    primary: Grid,
    alternate: Grid,
    cursor: Cursor,
    saved_cursor: [Cursor; 2],
    modes: Modes,
    scroll_top: usize,
    scroll_bottom: usize,
    tabs: Vec<bool>,
    title: String,
    title_stack: Vec<String>,
    cursor_style: CursorStyle,
    pub palette: Palette,
    /// OSC 8 targets; cell.link indexes this (1-based).
    pub hyperlinks: Vec<String>,
    keyboard_modes: [Vec<KeyboardModes>; 2],
    modify_other_keys: ModifyOtherKeys,
    responses: Vec<u8>,
    events: Vec<Event>,
    /// Pixel size of a cell, for XTWINOPS reports. Set by the host.
    pub cell_px: (u16, u16),
    max_scrollback: usize,
    /// Shell integration: marks on the primary screen, the reported cwd,
    /// and progress. Fed by OSC 133 / 7 / 9;4 before the bytes reach vte.
    pub marks: Vec<Mark>,
    pub cwd: Option<String>,
    pub progress: Option<(u8, u8)>,
    /// Bytes of an OSC that ended past the last chunk.
    pending_osc: Vec<u8>,
}

impl Term {
    pub fn new(cols: usize, rows: usize, max_scrollback: usize) -> Term {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Term {
            processor: Processor::new(),
            primary: Grid::new(cols, rows, max_scrollback),
            alternate: Grid::new(cols, rows, 0),
            cursor: Cursor::default(),
            saved_cursor: [Cursor::default(); 2],
            modes: Modes::AUTOWRAP | Modes::SHOW_CURSOR,
            scroll_top: 0,
            scroll_bottom: rows - 1,
            tabs: default_tabs(cols),
            title: String::new(),
            title_stack: Vec::new(),
            cursor_style: CursorStyle::default(),
            palette: Palette::new(),
            hyperlinks: Vec::new(),
            keyboard_modes: [Vec::new(), Vec::new()],
            modify_other_keys: ModifyOtherKeys::Reset,
            responses: Vec::new(),
            events: Vec::new(),
            cell_px: (8, 16),
            max_scrollback,
            marks: Vec::new(),
            cwd: None,
            progress: None,
            pending_osc: Vec::new(),
        }
    }

    /// Feed bytes from the PTY. OSC 133 / 7 / 9;4 are read here, at the
    /// point they occur, so a mark lands on the row the cursor is on when
    /// the shell sent it; the bytes still go to vte untouched.
    pub fn advance(&mut self, bytes: &[u8]) {
        if self.pending_osc.is_empty() {
            self.advance_scan(bytes);
        } else {
            let mut buf = std::mem::take(&mut self.pending_osc);
            buf.extend_from_slice(bytes);
            self.advance_scan(&buf);
        }
    }

    fn advance_scan(&mut self, bytes: &[u8]) {
        let mut start = 0;
        let mut i = 0;
        while i < bytes.len() {
            // ESC ] or C1 OSC.
            let osc_at = if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b']' {
                Some((i, i + 2))
            } else if bytes[i] == 0x9d {
                Some((i, i + 1))
            } else {
                None
            };
            let Some((at, body)) = osc_at else {
                i += 1;
                continue;
            };
            // Only the OSCs we care about; anything else passes straight through.
            let rest = &bytes[body..];
            let ours = rest.starts_with(b"133;") || rest.starts_with(b"7;") || rest.starts_with(b"9;4;");
            if !ours && rest.len() >= 4 {
                i += 1;
                continue;
            }
            // Find the terminator: BEL, ESC \, or C1 ST.
            let mut end = None;
            let mut j = body;
            while j < bytes.len() {
                match bytes[j] {
                    0x07 | 0x9c => {
                        end = Some((j, j + 1));
                        break;
                    }
                    0x1b if j + 1 < bytes.len() && bytes[j + 1] == b'\\' => {
                        end = Some((j, j + 2));
                        break;
                    }
                    0x1b => break, // another sequence began: not an OSC for us
                    _ => {}
                }
                j += 1;
            }
            let Some((pay_end, seq_end)) = end else {
                if j >= bytes.len() && (ours || rest.len() < 4) {
                    // Ends in a later chunk: feed what came before, keep the rest.
                    self.feed(&bytes[start..at]);
                    self.pending_osc = bytes[at..].to_vec();
                    return;
                }
                i += 1;
                continue;
            };
            if ours {
                self.feed(&bytes[start..at]);
                let payload = bytes[body..pay_end].to_vec();
                self.integration_osc(&payload);
                start = at;
            }
            i = seq_end;
        }
        self.feed(&bytes[start..]);
    }

    fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut processor = std::mem::take(&mut self.processor);
        processor.advance(self, bytes);
        self.processor = processor;
    }

    /// One of ours: 133;<A|B|C|D[;exit]>, 7;file://host/path, 9;4;state;pct.
    fn integration_osc(&mut self, payload: &[u8]) {
        let text = String::from_utf8_lossy(payload);
        if let Some(rest) = text.strip_prefix("133;") {
            if self.modes.contains(Modes::ALT_SCREEN) {
                return;
            }
            let mut parts = rest.split(';');
            let kind = match parts.next().and_then(|k| k.chars().next()) {
                Some('A') => MarkKind::PromptStart,
                Some('B') => MarkKind::CommandStart,
                Some('C') => MarkKind::OutputStart,
                Some('D') => MarkKind::CommandEnd(parts.next().and_then(|e| e.trim().parse().ok())),
                _ => return,
            };
            let line = self.primary.abs_row(self.cursor.row);
            let col = self.cursor.col;
            // A prompt redrawn on the same line replaces the last mark there.
            if let Some(last) = self.marks.last() {
                if last.line == line && last.kind == kind {
                    self.marks.pop();
                }
            }
            self.marks.push(Mark { line, col, kind });
            // Forget marks whose rows are gone.
            let oldest = self.primary.oldest_abs();
            if self.marks.first().is_some_and(|m| m.line < oldest) {
                self.marks.retain(|m| m.line >= oldest);
            }
            self.events.push(Event::Mark(kind));
        } else if let Some(rest) = text.strip_prefix("7;") {
            let url = rest.trim();
            // file://host/path → path; Windows drives come as /C:/…
            let path = url.strip_prefix("file://").map(|u| u.splitn(2, '/').nth(1).map(|p| format!("/{p}")).unwrap_or_default()).unwrap_or_else(|| url.to_string());
            let decoded = percent_decode(&path);
            let path = decoded.strip_prefix('/').filter(|p| p.len() > 1 && p.as_bytes()[1] == b':').map(|p| p.to_string()).unwrap_or(decoded);
            if !path.is_empty() {
                self.cwd = Some(path.clone());
                self.events.push(Event::Cwd(path));
            }
        } else if let Some(rest) = text.strip_prefix("9;4;") {
            let mut parts = rest.split(';');
            let state: u8 = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            let pct: u8 = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            self.progress = if state == 0 { None } else { Some((state, pct.min(100))) };
            self.events.push(Event::Progress(state, pct.min(100)));
        }
    }

    /// The output of a command: rows after its C mark up to its D mark
    /// (or the cursor's row when it is still running).
    pub fn output_text(&self, c: &Mark) -> String {
        let grid = &self.primary;
        let start = if c.col == 0 { c.line } else { c.line + 1 };
        let end = self
            .marks
            .iter()
            .find(|m| matches!(m.kind, MarkKind::CommandEnd(_)) && m.line >= c.line)
            .map(|m| m.line)
            .unwrap_or(grid.abs_row(self.cursor.row) + 1);
        let mut lines = Vec::new();
        let mut line = start;
        while line < end {
            if let Some(row) = grid.row_abs(line) {
                lines.push(row.text().trim_end().to_string());
            }
            line += 1;
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// Is the shell sitting at a prompt (the last mark is A or B)?
    pub fn at_prompt(&self) -> bool {
        matches!(self.marks.last().map(|m| m.kind), Some(MarkKind::PromptStart | MarkKind::CommandStart))
    }

    /// The command text between a B mark and the next C (or the cursor).
    pub fn command_text(&self, b: &Mark) -> String {
        let grid = &self.primary;
        let end = self
            .marks
            .iter()
            .find(|m| matches!(m.kind, MarkKind::OutputStart) && (m.line > b.line || (m.line == b.line && m.col >= b.col)))
            .map(|m| (m.line, m.col))
            .unwrap_or((grid.abs_row(self.cursor.row), self.cursor.col));
        let mut out = String::new();
        let mut line = b.line;
        while line <= end.0 {
            if let Some(row) = grid.row_abs(line) {
                let text: String = row.text();
                let from = if line == b.line { b.col } else { 0 };
                let to = if line == end.0 { end.1.min(text.chars().count()) } else { text.chars().count() };
                if to > from {
                    let piece: String = text.chars().skip(from).take(to - from).collect();
                    out.push_str(piece.trim_end());
                    if line < end.0 {
                        out.push('\n');
                    }
                }
            }
            line += 1;
        }
        out.trim().to_string()
    }

    /// Call periodically (e.g. once per frame): ends a synchronized update
    /// (mode 2026) whose application never sent the closing sequence.
    pub fn tick(&mut self) {
        let expired = self
            .processor
            .sync_timeout()
            .sync_timeout()
            .is_some_and(|t| std::time::Instant::now() >= t);
        if expired {
            let mut processor = std::mem::take(&mut self.processor);
            processor.stop_sync(self);
            self.processor = processor;
        }
    }

    /// Bytes the terminal wants written back to the PTY (DSR, DA, …).
    pub fn take_responses(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.responses)
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn grid(&self) -> &Grid {
        if self.modes.contains(Modes::ALT_SCREEN) {
            &self.alternate
        } else {
            &self.primary
        }
    }

    pub fn grid_mut(&mut self) -> &mut Grid {
        if self.modes.contains(Modes::ALT_SCREEN) {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }
    pub fn modes(&self) -> Modes {
        self.modes
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn cursor_style(&self) -> CursorStyle {
        self.cursor_style
    }
    pub fn cols(&self) -> usize {
        self.primary.cols()
    }
    pub fn rows(&self) -> usize {
        self.primary.rows()
    }
    pub fn scroll_region(&self) -> (usize, usize) {
        (self.scroll_top, self.scroll_bottom)
    }

    /// Active Kitty keyboard protocol flags.
    pub fn keyboard_mode(&self) -> KeyboardModes {
        self.keyboard_modes[self.screen_index()]
            .last()
            .copied()
            .unwrap_or(KeyboardModes::NO_MODE)
    }

    pub fn modify_other_keys(&self) -> ModifyOtherKeys {
        self.modify_other_keys
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if cols == self.cols() && rows == self.rows() {
            return;
        }
        let template = Cell::erased_from(&self.cursor.template);
        let cursor_row = self.cursor.row;
        let shift = self.primary.resize(cols, rows, &template, cursor_row);
        self.alternate.resize(cols, rows, &template, cursor_row);
        let shift = if self.modes.contains(Modes::ALT_SCREEN) {
            0
        } else {
            shift
        };
        self.cursor.row = (self.cursor.row as isize + shift).clamp(0, rows as isize - 1) as usize;
        self.cursor.col = self.cursor.col.min(cols - 1);
        self.cursor.wrap_next = false;
        for c in self.saved_cursor.iter_mut() {
            c.row = c.row.min(rows - 1);
            c.col = c.col.min(cols - 1);
        }
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        self.tabs = default_tabs(cols);
    }

    /// Full reset (RIS), keeping size.
    pub fn reset(&mut self) {
        let (cols, rows) = (self.cols(), self.rows());
        let cell_px = self.cell_px;
        let base = self.palette.clone();
        *self = Term::new(cols, rows, self.max_scrollback);
        self.cell_px = cell_px;
        self.palette = base;
        self.palette.reset_all_overrides();
        self.events.push(Event::ColorsChanged);
    }

    // --- internals -------------------------------------------------------

    fn screen_index(&self) -> usize {
        self.modes.contains(Modes::ALT_SCREEN) as usize
    }

    fn erased(&self) -> Cell {
        Cell::erased_from(&self.cursor.template)
    }

    fn respond(&mut self, s: impl AsRef<[u8]>) {
        self.responses.extend_from_slice(s.as_ref());
    }

    fn clamp_row(&self, row: isize) -> usize {
        let (min, max) = if self.modes.contains(Modes::ORIGIN) {
            (self.scroll_top as isize, self.scroll_bottom as isize)
        } else {
            (0, self.rows() as isize - 1)
        };
        row.clamp(min, max) as usize
    }

    fn set_cursor(&mut self, row: isize, col: isize) {
        self.cursor.row = self.clamp_row(row);
        self.cursor.col = col.clamp(0, self.cols() as isize - 1) as usize;
        self.cursor.wrap_next = false;
    }

    fn write_char(&mut self, c: char, width: usize) {
        let cols = self.cols();
        let template = self.cursor.template;
        if self.cursor.wrap_next {
            if self.modes.contains(Modes::AUTOWRAP) {
                let row = self.cursor.row;
                self.grid_mut().row_mut(row).wrapped = true;
                self.linefeed();
                self.cursor.col = 0;
            }
            self.cursor.wrap_next = false;
        }
        // A wide char that doesn't fit wraps early, leaving a blank.
        if width == 2 && self.cursor.col + 1 >= cols {
            if self.modes.contains(Modes::AUTOWRAP) {
                let row = self.cursor.row;
                let col = self.cursor.col;
                *self.grid_mut().cell_mut(row, col) = Cell::erased_from(&template);
                self.grid_mut().row_mut(row).wrapped = true;
                self.linefeed();
                self.cursor.col = 0;
            } else {
                return;
            }
        }
        let (row, col) = (self.cursor.row, self.cursor.col);
        if self.modes.contains(Modes::INSERT) {
            let erased = self.erased();
            let cells = &mut self.grid_mut().row_mut(row).cells;
            cells.truncate(cols - width);
            for _ in 0..width {
                cells.insert(col, erased);
            }
        }
        // Overwriting half of a wide char clears the other half.
        {
            let grid = self.grid_mut();
            let cur = *grid.cell(row, col);
            if cur.flags.contains(Flags::WIDE_SPACER) && col > 0 {
                let prev = grid.cell_mut(row, col - 1);
                *prev = Cell::erased_from(prev);
            }
            if cur.flags.contains(Flags::WIDE) && col + 1 < cols {
                let next = grid.cell_mut(row, col + 1);
                *next = Cell::erased_from(next);
            }
        }
        let mut cell = template;
        cell.ch = c;
        if width == 2 {
            cell.flags |= Flags::WIDE;
            let mut spacer = template;
            spacer.flags |= Flags::WIDE_SPACER;
            *self.grid_mut().cell_mut(row, col + 1) = spacer;
        }
        *self.grid_mut().cell_mut(row, col) = cell;

        if col + width >= cols {
            self.cursor.col = cols - 1;
            self.cursor.wrap_next = true;
        } else {
            self.cursor.col = col + width;
        }
    }

    fn swap_screen(&mut self, to_alt: bool) {
        if to_alt == self.modes.contains(Modes::ALT_SCREEN) {
            return;
        }
        self.modes.set(Modes::ALT_SCREEN, to_alt);
        if to_alt {
            let erased = self.erased();
            self.alternate.clear_all(&erased);
        }
        self.cursor.wrap_next = false;
        self.grid_mut().damage_all();
    }

    fn set_private_mode_inner(&mut self, mode: PrivateMode, on: bool) {
        use NamedPrivateMode as M;
        let named = match mode {
            PrivateMode::Named(n) => n,
            PrivateMode::Unknown(47) | PrivateMode::Unknown(1047) => {
                self.swap_screen(on);
                return;
            }
            PrivateMode::Unknown(1048) => {
                if on {
                    self.save_cursor_position();
                } else {
                    self.restore_cursor_position();
                }
                return;
            }
            PrivateMode::Unknown(_) => return,
        };
        match named {
            M::CursorKeys => self.modes.set(Modes::APP_CURSOR, on),
            M::ColumnMode => {
                // DECCOLM: we don't do 132 columns, but the side effects apply.
                let erased = self.erased();
                self.grid_mut().clear_all(&erased);
                self.scroll_top = 0;
                self.scroll_bottom = self.rows() - 1;
                self.set_cursor(0, 0);
            }
            M::Origin => {
                self.modes.set(Modes::ORIGIN, on);
                self.set_cursor(0, 0);
            }
            M::LineWrap => self.modes.set(Modes::AUTOWRAP, on),
            M::BlinkingCursor => self.modes.set(Modes::BLINK_CURSOR, on),
            M::ShowCursor => self.modes.set(Modes::SHOW_CURSOR, on),
            M::ReportMouseClicks => self.modes.set(Modes::MOUSE_CLICK, on),
            M::ReportCellMouseMotion => self.modes.set(Modes::MOUSE_MOTION, on),
            M::ReportAllMouseMotion => self.modes.set(Modes::MOUSE_ANY, on),
            M::ReportFocusInOut => self.modes.set(Modes::FOCUS_EVENTS, on),
            M::Utf8Mouse => self.modes.set(Modes::MOUSE_UTF8, on),
            M::SgrMouse => self.modes.set(Modes::MOUSE_SGR, on),
            M::AlternateScroll => self.modes.set(Modes::ALTERNATE_SCROLL, on),
            M::UrgencyHints => {}
            M::SwapScreenAndSetRestoreCursor => {
                if on {
                    self.save_cursor_position();
                    self.swap_screen(true);
                } else {
                    self.swap_screen(false);
                    self.restore_cursor_position();
                }
            }
            M::BracketedPaste => self.modes.set(Modes::BRACKETED_PASTE, on),
            M::SyncUpdate => {} // handled inside vte's Processor
        }
    }

    fn private_mode_state(&self, mode: PrivateMode) -> u8 {
        use NamedPrivateMode as M;
        let flag = match mode {
            PrivateMode::Named(n) => match n {
                M::CursorKeys => Modes::APP_CURSOR,
                M::Origin => Modes::ORIGIN,
                M::LineWrap => Modes::AUTOWRAP,
                M::BlinkingCursor => Modes::BLINK_CURSOR,
                M::ShowCursor => Modes::SHOW_CURSOR,
                M::ReportMouseClicks => Modes::MOUSE_CLICK,
                M::ReportCellMouseMotion => Modes::MOUSE_MOTION,
                M::ReportAllMouseMotion => Modes::MOUSE_ANY,
                M::ReportFocusInOut => Modes::FOCUS_EVENTS,
                M::Utf8Mouse => Modes::MOUSE_UTF8,
                M::SgrMouse => Modes::MOUSE_SGR,
                M::AlternateScroll => Modes::ALTERNATE_SCROLL,
                M::SwapScreenAndSetRestoreCursor => Modes::ALT_SCREEN,
                M::BracketedPaste => Modes::BRACKETED_PASTE,
                M::ColumnMode | M::UrgencyHints => return 4, // permanently reset
                M::SyncUpdate => return 2,
            },
            PrivateMode::Unknown(47) | PrivateMode::Unknown(1047) => Modes::ALT_SCREEN,
            PrivateMode::Unknown(_) => return 0,
        };
        if self.modes.contains(flag) {
            1
        } else {
            2
        }
    }

    fn scroll_up_region(&mut self, n: usize) {
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        let erased = self.erased();
        let keep = !self.modes.contains(Modes::ALT_SCREEN);
        self.grid_mut().scroll_up(top, bottom, n, &erased, keep);
    }

    fn scroll_down_region(&mut self, n: usize) {
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        let erased = self.erased();
        self.grid_mut().scroll_down(top, bottom, n, &erased);
    }

    fn set_color_index(&mut self, index: usize, rgb: Option<Rgb>) {
        match rgb {
            Some(rgb) => self.palette.set_override(index, rgb),
            None => self.palette.reset_override(index),
        }
        self.events.push(Event::ColorsChanged);
    }
}

fn default_tabs(cols: usize) -> Vec<bool> {
    (0..cols).map(|c| c % 8 == 0).collect()
}

fn convert_color(c: ansi::Color) -> Color {
    match c {
        ansi::Color::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        ansi::Color::Indexed(i) => Color::Indexed(i),
        ansi::Color::Named(n) => match n as usize {
            i @ 0..=15 => Color::Indexed(i as u8),
            i if i == NamedColor::Foreground as usize || i == NamedColor::Background as usize => {
                Color::Default
            }
            i if i == NamedColor::BrightForeground as usize => Color::Default,
            i if i == NamedColor::DimForeground as usize => Color::Default,
            // DimBlack..DimWhite map back to their base color; dimming is a flag.
            i if i >= NamedColor::DimBlack as usize && i <= NamedColor::DimWhite as usize => {
                Color::Indexed((i - NamedColor::DimBlack as usize) as u8)
            }
            _ => Color::Default,
        },
    }
}

impl Handler for Term {
    fn set_title(&mut self, title: Option<String>) {
        self.title = title.unwrap_or_default();
        self.events.push(Event::Title(self.title.clone()));
    }

    fn set_cursor_style(&mut self, style: Option<CursorStyle>) {
        self.cursor_style = style.unwrap_or_default();
        self.events.push(Event::CursorStyle(self.cursor_style));
    }

    fn set_cursor_shape(&mut self, shape: CursorShape) {
        self.cursor_style.shape = shape;
        self.events.push(Event::CursorStyle(self.cursor_style));
    }

    fn input(&mut self, c: char) {
        let c = self.cursor.charsets[self.cursor.charset as usize].map(c);
        let width = match c.width() {
            // Zero-width (combining) marks: grapheme clusters are a v1 concern.
            Some(0) => return,
            Some(w) => w.min(2),
            None => return, // control chars come through `execute`
        };
        self.write_char(c, width);
    }

    fn goto(&mut self, line: i32, col: usize) {
        let line = line as isize
            + if self.modes.contains(Modes::ORIGIN) {
                self.scroll_top as isize
            } else {
                0
            };
        self.set_cursor(line, col as isize);
    }

    fn goto_line(&mut self, line: i32) {
        let col = self.cursor.col;
        self.goto(line, col);
    }

    fn goto_col(&mut self, col: usize) {
        self.cursor.col = col.min(self.cols() - 1);
        self.cursor.wrap_next = false;
    }

    fn insert_blank(&mut self, n: usize) {
        let (row, col) = (self.cursor.row, self.cursor.col);
        let cols = self.cols();
        let n = n.min(cols - col);
        let erased = self.erased();
        let cells = &mut self.grid_mut().row_mut(row).cells;
        cells.truncate(cols - n);
        for _ in 0..n {
            cells.insert(col, erased);
        }
        self.cursor.wrap_next = false;
    }

    fn move_up(&mut self, n: usize) {
        let min = if self.cursor.row >= self.scroll_top {
            self.scroll_top
        } else {
            0
        };
        self.cursor.row = self.cursor.row.saturating_sub(n).max(min);
        self.cursor.wrap_next = false;
    }

    fn move_down(&mut self, n: usize) {
        let max = if self.cursor.row <= self.scroll_bottom {
            self.scroll_bottom
        } else {
            self.rows() - 1
        };
        self.cursor.row = (self.cursor.row + n).min(max);
        self.cursor.wrap_next = false;
    }

    fn identify_terminal(&mut self, intermediate: Option<char>) {
        match intermediate {
            None => self.respond(b"\x1b[?62;22c"), // VT220 with ANSI color
            Some('>') => self.respond(b"\x1b[>1;10;0c"),
            _ => {}
        }
    }

    fn device_status(&mut self, arg: usize) {
        match arg {
            5 => self.respond(b"\x1b[0n"),
            6 => {
                let row = if self.modes.contains(Modes::ORIGIN) {
                    self.cursor.row - self.scroll_top
                } else {
                    self.cursor.row
                };
                self.respond(format!("\x1b[{};{}R", row + 1, self.cursor.col + 1));
            }
            _ => {}
        }
    }

    fn move_forward(&mut self, n: usize) {
        self.cursor.col = (self.cursor.col + n).min(self.cols() - 1);
        self.cursor.wrap_next = false;
    }

    fn move_backward(&mut self, n: usize) {
        self.cursor.col = self.cursor.col.saturating_sub(n);
        self.cursor.wrap_next = false;
    }

    fn move_down_and_cr(&mut self, n: usize) {
        self.move_down(n);
        self.cursor.col = 0;
    }

    fn move_up_and_cr(&mut self, n: usize) {
        self.move_up(n);
        self.cursor.col = 0;
    }

    fn put_tab(&mut self, count: u16) {
        let cols = self.cols();
        for _ in 0..count {
            let mut col = self.cursor.col + 1;
            while col < cols && !self.tabs[col] {
                col += 1;
            }
            self.cursor.col = col.min(cols - 1);
            if col >= cols {
                break;
            }
        }
        self.cursor.wrap_next = false;
    }

    fn backspace(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        }
        self.cursor.wrap_next = false;
    }

    fn carriage_return(&mut self) {
        self.cursor.col = 0;
        self.cursor.wrap_next = false;
    }

    fn linefeed(&mut self) {
        if self.cursor.row == self.scroll_bottom {
            self.scroll_up_region(1);
        } else if self.cursor.row < self.rows() - 1 {
            self.cursor.row += 1;
        }
        if self.modes.contains(Modes::LINEFEED_NEWLINE) {
            self.cursor.col = 0;
        }
        self.cursor.wrap_next = false;
    }

    fn bell(&mut self) {
        self.events.push(Event::Bell);
    }

    fn newline(&mut self) {
        self.linefeed();
        self.cursor.col = 0;
    }

    fn set_horizontal_tabstop(&mut self) {
        let col = self.cursor.col;
        self.tabs[col] = true;
    }

    fn scroll_up(&mut self, n: usize) {
        self.scroll_up_region(n);
    }

    fn scroll_down(&mut self, n: usize) {
        self.scroll_down_region(n);
    }

    fn insert_blank_lines(&mut self, n: usize) {
        let row = self.cursor.row;
        if row >= self.scroll_top && row <= self.scroll_bottom {
            let bottom = self.scroll_bottom;
            let erased = self.erased();
            self.grid_mut().scroll_down(row, bottom, n, &erased);
        }
        self.cursor.col = 0;
        self.cursor.wrap_next = false;
    }

    fn delete_lines(&mut self, n: usize) {
        let row = self.cursor.row;
        if row >= self.scroll_top && row <= self.scroll_bottom {
            let bottom = self.scroll_bottom;
            let erased = self.erased();
            self.grid_mut().scroll_up(row, bottom, n, &erased, false);
        }
        self.cursor.col = 0;
        self.cursor.wrap_next = false;
    }

    fn erase_chars(&mut self, n: usize) {
        let (row, col) = (self.cursor.row, self.cursor.col);
        let end = (col + n).min(self.cols());
        let erased = self.erased();
        let r = self.grid_mut().row_mut(row);
        for c in col..end {
            r.cells[c] = erased;
        }
        self.cursor.wrap_next = false;
    }

    fn delete_chars(&mut self, n: usize) {
        let (row, col) = (self.cursor.row, self.cursor.col);
        let cols = self.cols();
        let n = n.min(cols - col);
        let erased = self.erased();
        let cells = &mut self.grid_mut().row_mut(row).cells;
        cells.drain(col..col + n);
        cells.resize(cols, erased);
        self.cursor.wrap_next = false;
    }

    fn move_backward_tabs(&mut self, count: u16) {
        for _ in 0..count {
            let mut col = self.cursor.col;
            while col > 0 {
                col -= 1;
                if self.tabs[col] {
                    break;
                }
            }
            self.cursor.col = col;
        }
        self.cursor.wrap_next = false;
    }

    fn move_forward_tabs(&mut self, count: u16) {
        self.put_tab(count);
    }

    fn save_cursor_position(&mut self) {
        let i = self.screen_index();
        self.saved_cursor[i] = self.cursor;
    }

    fn restore_cursor_position(&mut self) {
        let i = self.screen_index();
        self.cursor = self.saved_cursor[i];
        self.cursor.row = self.cursor.row.min(self.rows() - 1);
        self.cursor.col = self.cursor.col.min(self.cols() - 1);
        self.cursor.wrap_next = false;
    }

    fn clear_line(&mut self, mode: LineClearMode) {
        let (row, col) = (self.cursor.row, self.cursor.col);
        let cols = self.cols();
        let erased = self.erased();
        let range = match mode {
            LineClearMode::Right => col..cols,
            LineClearMode::Left => 0..col + 1,
            LineClearMode::All => 0..cols,
        };
        let r = self.grid_mut().row_mut(row);
        for c in range {
            r.cells[c] = erased;
        }
        if matches!(mode, LineClearMode::Right | LineClearMode::All) {
            r.wrapped = false;
        }
    }

    fn clear_screen(&mut self, mode: ClearMode) {
        let (row, col) = (self.cursor.row, self.cursor.col);
        let (rows, cols) = (self.rows(), self.cols());
        let erased = self.erased();
        match mode {
            ClearMode::Below => {
                let grid = self.grid_mut();
                for c in col..cols {
                    grid.row_mut(row).cells[c] = erased;
                }
                for r in row + 1..rows {
                    *grid.row_mut(r) = crate::grid::Row::blank(cols, &erased);
                }
            }
            ClearMode::Above => {
                let grid = self.grid_mut();
                for r in 0..row {
                    *grid.row_mut(r) = crate::grid::Row::blank(cols, &erased);
                }
                for c in 0..=col.min(cols - 1) {
                    grid.row_mut(row).cells[c] = erased;
                }
            }
            ClearMode::All => self.grid_mut().clear_all(&erased),
            ClearMode::Saved => self.grid_mut().clear_scrollback(),
        }
    }

    fn clear_tabs(&mut self, mode: TabulationClearMode) {
        match mode {
            TabulationClearMode::Current => {
                let col = self.cursor.col;
                self.tabs[col] = false;
            }
            TabulationClearMode::All => self.tabs.iter_mut().for_each(|t| *t = false),
        }
    }

    fn set_tabs(&mut self, interval: u16) {
        let interval = interval.max(1) as usize;
        for (c, t) in self.tabs.iter_mut().enumerate() {
            *t = c % interval == 0;
        }
    }

    fn reset_state(&mut self) {
        self.reset();
    }

    fn reverse_index(&mut self) {
        if self.cursor.row == self.scroll_top {
            self.scroll_down_region(1);
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
        }
        self.cursor.wrap_next = false;
    }

    fn terminal_attribute(&mut self, attr: Attr) {
        let t = &mut self.cursor.template;
        let f = &mut t.flags;
        match attr {
            Attr::Reset => {
                *t = Cell {
                    link: t.link,
                    ..Cell::default()
                };
            }
            Attr::Bold => f.insert(Flags::BOLD),
            Attr::Dim => f.insert(Flags::DIM),
            Attr::Italic => f.insert(Flags::ITALIC),
            Attr::Underline => {
                f.remove(Flags::ANY_UNDERLINE);
                f.insert(Flags::UNDERLINE);
            }
            Attr::DoubleUnderline => {
                f.remove(Flags::ANY_UNDERLINE);
                f.insert(Flags::DOUBLE_UL);
            }
            Attr::Undercurl => {
                f.remove(Flags::ANY_UNDERLINE);
                f.insert(Flags::UNDERCURL);
            }
            Attr::DottedUnderline => {
                f.remove(Flags::ANY_UNDERLINE);
                f.insert(Flags::DOTTED_UL);
            }
            Attr::DashedUnderline => {
                f.remove(Flags::ANY_UNDERLINE);
                f.insert(Flags::DASHED_UL);
            }
            Attr::BlinkSlow | Attr::BlinkFast => f.insert(Flags::BLINK),
            Attr::Reverse => f.insert(Flags::INVERSE),
            Attr::Hidden => f.insert(Flags::HIDDEN),
            Attr::Strike => f.insert(Flags::STRIKE),
            Attr::CancelBold => f.remove(Flags::BOLD),
            Attr::CancelBoldDim => f.remove(Flags::BOLD | Flags::DIM),
            Attr::CancelItalic => f.remove(Flags::ITALIC),
            Attr::CancelUnderline => f.remove(Flags::ANY_UNDERLINE),
            Attr::CancelBlink => f.remove(Flags::BLINK),
            Attr::CancelReverse => f.remove(Flags::INVERSE),
            Attr::CancelHidden => f.remove(Flags::HIDDEN),
            Attr::CancelStrike => f.remove(Flags::STRIKE),
            Attr::Foreground(c) => t.fg = convert_color(c),
            Attr::Background(c) => t.bg = convert_color(c),
            Attr::UnderlineColor(c) => t.ul = c.map(convert_color),
        }
    }

    fn set_mode(&mut self, mode: Mode) {
        match mode {
            Mode::Named(NamedMode::Insert) => self.modes.insert(Modes::INSERT),
            Mode::Named(NamedMode::LineFeedNewLine) => self.modes.insert(Modes::LINEFEED_NEWLINE),
            Mode::Unknown(_) => {}
        }
    }

    fn unset_mode(&mut self, mode: Mode) {
        match mode {
            Mode::Named(NamedMode::Insert) => self.modes.remove(Modes::INSERT),
            Mode::Named(NamedMode::LineFeedNewLine) => self.modes.remove(Modes::LINEFEED_NEWLINE),
            Mode::Unknown(_) => {}
        }
    }

    fn report_mode(&mut self, mode: Mode) {
        let state = match mode {
            Mode::Named(NamedMode::Insert) => 1 + !self.modes.contains(Modes::INSERT) as u8,
            Mode::Named(NamedMode::LineFeedNewLine) => {
                1 + !self.modes.contains(Modes::LINEFEED_NEWLINE) as u8
            }
            Mode::Unknown(_) => 0,
        };
        self.respond(format!("\x1b[{};{}$y", mode.raw(), state));
    }

    fn set_private_mode(&mut self, mode: PrivateMode) {
        self.set_private_mode_inner(mode, true);
    }

    fn unset_private_mode(&mut self, mode: PrivateMode) {
        self.set_private_mode_inner(mode, false);
    }

    fn report_private_mode(&mut self, mode: PrivateMode) {
        let state = self.private_mode_state(mode);
        self.respond(format!("\x1b[?{};{}$y", mode.raw(), state));
    }

    fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>) {
        let rows = self.rows();
        let top = top.max(1) - 1;
        let bottom = bottom.unwrap_or(rows).min(rows) - 1;
        if top < bottom {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        } else {
            self.scroll_top = 0;
            self.scroll_bottom = rows - 1;
        }
        self.goto(0, 0);
    }

    fn set_keypad_application_mode(&mut self) {
        self.modes.insert(Modes::APP_KEYPAD);
    }

    fn unset_keypad_application_mode(&mut self) {
        self.modes.remove(Modes::APP_KEYPAD);
    }

    fn set_active_charset(&mut self, index: CharsetIndex) {
        self.cursor.charset = index;
    }

    fn configure_charset(&mut self, index: CharsetIndex, charset: StandardCharset) {
        self.cursor.charsets[index as usize] = charset;
    }

    fn set_color(&mut self, index: usize, color: ansi::Rgb) {
        self.set_color_index(index, Some(color.into()));
    }

    fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str) {
        let c = self.palette.get(index);
        self.respond(format!(
            "\x1b]{};rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}{}",
            prefix, c.r, c.r, c.g, c.g, c.b, c.b, terminator
        ));
    }

    fn reset_color(&mut self, index: usize) {
        self.set_color_index(index, None);
    }

    fn clipboard_store(&mut self, clipboard: u8, base64: &[u8]) {
        self.events
            .push(Event::ClipboardStore(clipboard, base64.to_vec()));
    }

    fn clipboard_load(&mut self, clipboard: u8, _terminator: &str) {
        self.events.push(Event::ClipboardLoad(clipboard));
    }

    fn decaln(&mut self) {
        let (rows, cols) = (self.rows(), self.cols());
        let cell = Cell {
            ch: 'E',
            ..Cell::default()
        };
        let grid = self.grid_mut();
        for r in 0..rows {
            let row = grid.row_mut(r);
            for c in 0..cols {
                row.cells[c] = cell;
            }
        }
    }

    fn push_title(&mut self) {
        self.title_stack.push(self.title.clone());
        self.title_stack.truncate(10);
    }

    fn pop_title(&mut self) {
        if let Some(t) = self.title_stack.pop() {
            self.set_title(Some(t));
        }
    }

    fn text_area_size_pixels(&mut self) {
        let (w, h) = (
            self.cols() as u32 * self.cell_px.0 as u32,
            self.rows() as u32 * self.cell_px.1 as u32,
        );
        self.respond(format!("\x1b[4;{};{}t", h, w));
    }

    fn text_area_size_chars(&mut self) {
        self.respond(format!("\x1b[8;{};{}t", self.rows(), self.cols()));
    }

    fn set_hyperlink(&mut self, link: Option<Hyperlink>) {
        self.cursor.template.link = match link {
            Some(h) => {
                self.hyperlinks.push(h.uri);
                self.hyperlinks.len() as u32
            }
            None => 0,
        };
    }

    fn report_keyboard_mode(&mut self) {
        let mode = self.keyboard_mode().bits();
        self.respond(format!("\x1b[?{}u", mode));
    }

    fn push_keyboard_mode(&mut self, mode: KeyboardModes) {
        let stack = &mut self.keyboard_modes[self.modes.contains(Modes::ALT_SCREEN) as usize];
        stack.push(mode);
        if stack.len() > 32 {
            stack.remove(0);
        }
    }

    fn pop_keyboard_modes(&mut self, to_pop: u16) {
        let stack = &mut self.keyboard_modes[self.modes.contains(Modes::ALT_SCREEN) as usize];
        let n = (to_pop.max(1) as usize).min(stack.len());
        stack.truncate(stack.len() - n);
    }

    fn set_keyboard_mode(&mut self, mode: KeyboardModes, behavior: KeyboardModesApplyBehavior) {
        let stack = &mut self.keyboard_modes[self.modes.contains(Modes::ALT_SCREEN) as usize];
        let current = stack.last().copied().unwrap_or(KeyboardModes::NO_MODE);
        let new = match behavior {
            KeyboardModesApplyBehavior::Replace => mode,
            KeyboardModesApplyBehavior::Union => current | mode,
            KeyboardModesApplyBehavior::Difference => current & !mode,
        };
        match stack.last_mut() {
            Some(top) => *top = new,
            None => stack.push(new),
        }
    }

    fn set_modify_other_keys(&mut self, mode: ModifyOtherKeys) {
        self.modify_other_keys = mode;
    }

    fn report_modify_other_keys(&mut self) {
        let v = match self.modify_other_keys {
            ModifyOtherKeys::Reset => 0,
            ModifyOtherKeys::EnableExceptWellDefined => 1,
            ModifyOtherKeys::EnableAll => 2,
        };
        self.respond(format!("\x1b[>4;{}m", v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(cols: usize, rows: usize) -> Term {
        Term::new(cols, rows, 100)
    }

    fn feed(t: &mut Term, s: &str) {
        t.advance(s.as_bytes());
    }

    #[test]
    fn marks_land_on_their_rows_and_survive_scrolling() {
        let mut t = term(20, 3);
        feed(&mut t, "\x1b]133;A\x07$ \x1b]133;B\x07ls -la\r\n\x1b]133;C\x07a\r\nb\r\nc\r\n\x1b]133;D;0\x07");
        let kinds: Vec<MarkKind> = t.marks.iter().map(|m| m.kind).collect();
        assert_eq!(kinds, vec![MarkKind::PromptStart, MarkKind::CommandStart, MarkKind::OutputStart, MarkKind::CommandEnd(Some(0))]);
        // The prompt was on absolute line 0, which has scrolled into history.
        assert_eq!(t.marks[0].line, 0);
        assert_eq!(t.marks[1].col, 2);
        assert!(matches!(t.grid().locate(0), Some(crate::grid::Loc::History(_))));
        assert_eq!(t.command_text(&t.marks[1].clone()), "ls -la");
        assert_eq!(t.grid().row_abs(0).map(|r| r.text().trim_end().to_string()), Some("$ ls -la".into()));
    }

    #[test]
    fn osc_split_across_chunks_and_cwd_and_progress() {
        let mut t = term(20, 3);
        feed(&mut t, "x\x1b]7;file://pc/C:/Users/seb/nus");
        assert!(t.cwd.is_none());
        feed(&mut t, "\x07y\x1b]9;4;1;42\x1b\\z");
        assert_eq!(t.cwd.as_deref(), Some("C:/Users/seb/nus"));
        assert_eq!(t.progress, Some((1, 42)));
        assert_eq!(t.grid().row(0).text().trim_end(), "xyz");
        let events = t.take_events();
        assert!(events.iter().any(|e| matches!(e, Event::Cwd(_))));
        assert!(events.iter().any(|e| matches!(e, Event::Progress(1, 42))));
    }

    #[test]
    fn foreign_oscs_pass_through() {
        let mut t = term(20, 3);
        feed(&mut t, "\x1b]0;my title\x07hi");
        assert_eq!(t.grid().row(0).text().trim_end(), "hi");
        assert!(t.marks.is_empty());
    }

    #[test]
    fn prints_and_wraps() {
        let mut t = term(5, 2);
        feed(&mut t, "abcdefg");
        assert_eq!(t.grid().text(), "abcde\nfg");
        assert_eq!((t.cursor().row, t.cursor().col), (1, 2));
        assert!(t.grid().row(0).wrapped);
    }

    #[test]
    fn crlf_and_scroll_into_history() {
        let mut t = term(4, 2);
        feed(&mut t, "a\r\nb\r\nc");
        assert_eq!(t.grid().text(), "b\nc");
        assert_eq!(t.grid().scrollback_len(), 1);
        assert_eq!(t.grid().visible_row(0).text(), "b");
        t.grid_mut().scroll_display(1);
        assert_eq!(t.grid().visible_row(0).text(), "a");
        assert_eq!(t.grid().visible_row(1).text(), "b");
    }

    #[test]
    fn cursor_movement_and_erase() {
        let mut t = term(10, 3);
        feed(&mut t, "hello world");
        feed(&mut t, "\x1b[1;1H\x1b[K"); // home, erase to EOL
        assert_eq!(t.grid().text(), "\nd\n");
        feed(&mut t, "\x1b[2J");
        assert_eq!(t.grid().text(), "\n\n");
        feed(&mut t, "\x1b[2;3Hx");
        assert_eq!(t.grid().cell(1, 2).ch, 'x');
    }

    #[test]
    fn sgr_attributes_and_colors() {
        let mut t = term(10, 1);
        feed(&mut t, "\x1b[1;31;48;2;1;2;3mA\x1b[0mB");
        let a = t.grid().cell(0, 0);
        assert!(a.flags.contains(Flags::BOLD));
        assert_eq!(a.fg, Color::Indexed(1));
        assert_eq!(a.bg, Color::Rgb(1, 2, 3));
        let b = t.grid().cell(0, 1);
        assert_eq!(
            *b,
            Cell {
                ch: 'B',
                ..Cell::default()
            }
        );
    }

    #[test]
    fn scroll_region_and_reverse_index() {
        let mut t = term(3, 4);
        feed(&mut t, "1\r\n2\r\n3\r\n4");
        feed(&mut t, "\x1b[2;3r"); // region rows 2..3, cursor homes to (0,0)
        feed(&mut t, "\x1b[2;1H\x1bM"); // to row 2 (region top), RI scrolls region down
        assert_eq!(t.grid().text(), "1\n\n2\n4");
        assert_eq!(t.grid().scrollback_len(), 0);
    }

    #[test]
    fn alt_screen_round_trip() {
        let mut t = term(5, 2);
        feed(&mut t, "main");
        feed(&mut t, "\x1b[?1049h");
        assert!(t.modes().contains(Modes::ALT_SCREEN));
        assert_eq!(t.grid().text(), "\n");
        feed(&mut t, "alt");
        feed(&mut t, "\x1b[?1049l");
        assert_eq!(
            t.grid().text(),
            "main
"
        );
        assert_eq!(t.cursor().col, 4);
    }

    #[test]
    fn wide_chars_take_two_cells() {
        let mut t = term(4, 1);
        feed(&mut t, "a漢b");
        assert_eq!(t.grid().cell(0, 1).ch, '漢');
        assert!(t.grid().cell(0, 1).flags.contains(Flags::WIDE));
        assert!(t.grid().cell(0, 2).flags.contains(Flags::WIDE_SPACER));
        assert_eq!(t.grid().cell(0, 3).ch, 'b');
        assert_eq!(t.grid().row(0).text(), "a漢b");
    }

    #[test]
    fn insert_delete_chars_and_lines() {
        let mut t = term(6, 3);
        feed(&mut t, "abcdef\x1b[1;3H\x1b[2@"); // ICH 2 at col 3
        assert_eq!(t.grid().row(0).text(), "ab  cd");
        feed(&mut t, "\x1b[1;1H\x1b[3P"); // DCH 3
        assert_eq!(t.grid().row(0).text(), " cd");
        feed(&mut t, "\x1b[2;1Hx\x1b[3;1Hy\x1b[1;1H\x1b[M"); // DL at row 1
        assert_eq!(t.grid().text(), "x\ny\n");
    }

    #[test]
    fn responses_dsr_and_da() {
        let mut t = term(10, 5);
        feed(&mut t, "\x1b[3;4H\x1b[6n\x1b[c");
        assert_eq!(t.take_responses(), b"\x1b[3;4R\x1b[?62;22c");
    }

    #[test]
    fn kitty_keyboard_stack() {
        let mut t = term(4, 1);
        feed(&mut t, "\x1b[>1u\x1b[?u");
        assert_eq!(t.keyboard_mode(), KeyboardModes::DISAMBIGUATE_ESC_CODES);
        assert_eq!(t.take_responses(), b"\x1b[?1u");
        feed(&mut t, "\x1b[<u");
        assert_eq!(t.keyboard_mode(), KeyboardModes::NO_MODE);
    }

    #[test]
    fn resize_keeps_cursor_line() {
        let mut t = term(10, 4);
        feed(&mut t, "a\r\nb\r\nc\r\nd"); // cursor on row 3
        t.resize(10, 2);
        assert_eq!(t.grid().text(), "c\nd");
        assert_eq!(t.cursor().row, 1);
        t.resize(10, 4);
        assert_eq!(t.grid().text(), "a\nb\nc\nd");
        assert_eq!(t.cursor().row, 3);
    }

    #[test]
    fn shrink_drops_blank_rows_below_cursor_first() {
        let mut t = term(10, 4);
        feed(&mut t, "prompt>"); // cursor on row 0, rows 1..3 blank
        t.resize(10, 2);
        assert_eq!(
            t.grid().text(),
            "prompt>
"
        );
        assert_eq!(t.cursor().row, 0);
        assert_eq!(t.grid().scrollback_len(), 0);
    }

    #[test]
    fn title_and_bell_events() {
        let mut t = term(4, 1);
        feed(&mut t, "\x1b]0;hi\x07\x07");
        assert_eq!(
            t.take_events(),
            vec![Event::Title("hi".into()), Event::Bell]
        );
    }

    #[test]
    fn synchronized_update_buffers_until_end() {
        let mut t = term(4, 1);
        feed(&mut t, "\x1b[?2026hab");
        assert_eq!(t.grid().text(), "");
        feed(&mut t, "\x1b[?2026l");
        assert_eq!(t.grid().text(), "ab");
    }
}

/// Minimal %XX decoding for OSC 7 paths.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}
