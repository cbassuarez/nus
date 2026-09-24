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
use crate::utf8::Utf8Prefix;

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
        /// DECSET 1016: SGR reports in pixels, not cells.
        const MOUSE_SGR_PIXEL   = 1 << 17;

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
    discard_control: bool,
    discard_escape: bool,
    discard_utf8: Utf8Prefix,
    hyperlink_bytes: usize,
    /// Transport state, separate from terminal modes and the vte decoder.
    scan_utf8: Utf8Prefix,
    feed_utf8: Utf8Prefix,
    /// Terminal images: decoded once, placed at absolute lines.
    pub images: Vec<crate::images::Image>,
    pub placements: Vec<crate::images::Placement>,
    /// A chunked Kitty transmission still arriving.
    pending_image: Option<crate::images::Pending>,
    /// Bumps when images or placements change, so the host re-syncs textures.
    pub images_gen: u64,
    next_image_id: u32,
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
            discard_control: false,
            discard_escape: false,
            discard_utf8: Utf8Prefix::default(),
            hyperlink_bytes: 0,
            scan_utf8: Utf8Prefix::default(),
            feed_utf8: Utf8Prefix::default(),
            images: Vec::new(),
            placements: Vec::new(),
            pending_image: None,
            images_gen: 0,
            next_image_id: 1 << 24,
        }
    }

    /// Feed bytes from the PTY. OSC 133 / 7 / 9;4 are read here, at the
    /// point they occur, so a mark lands on the row the cursor is on when
    /// the shell sent it; the bytes still go to vte untouched.
    pub fn advance(&mut self, bytes: &[u8]) {
        // Bound both complete and split control strings, including callers
        // feeding an entire replay at once. Never render an oversized payload
        // as shell text: discard through its terminator, then resume parsing.
        for chunk in bytes.chunks(16 * 1024) {
            let mut offset = 0;
            while self.discard_control && offset < chunk.len() {
                let b = chunk[offset];
                let text = self.discard_utf8.observe(b);
                if !text
                    && (matches!(b, 7 | 0x9c | 0x18 | 0x1a) || (self.discard_escape && b == b'\\'))
                {
                    self.discard_control = false;
                    self.scan_utf8 = Utf8Prefix::default();
                }
                self.discard_escape = b == 0x1b;
                offset += 1;
            }
            if offset < chunk.len() {
                self.advance_chunk(&chunk[offset..]);
                let image = self.pending_osc.starts_with(b"\x1b_G")
                    || self.pending_osc.starts_with(b"\x1bP")
                    || self.pending_osc.starts_with(b"\x1b]1337;")
                    || self.pending_osc.starts_with(b"\x9d1337;")
                    || self.pending_osc.starts_with(b"\x1b]52;")
                    || self.pending_osc.starts_with(b"\x9d52;");
                let limit = if image {
                    crate::images::MAX_ENCODED_BYTES
                } else {
                    64 * 1024
                };
                if self.pending_osc.len() > limit {
                    self.discard_escape = self.pending_osc.last() == Some(&0x1b);
                    self.discard_utf8 = Utf8Prefix::at_end(&self.pending_osc);
                    self.discard_control = true;
                    self.pending_osc = Vec::new();
                }
            }
        }
    }

    fn advance_chunk(&mut self, bytes: &[u8]) {
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
            // C1-valued bytes inside UTF-8 (e.g. 面 = e9 9d a2) are text,
            // not OSC introducers. Carry this boundary state across PTY reads.
            if self.scan_utf8.observe(bytes[i]) {
                i += 1;
                continue;
            }
            // Keep a split ESC introducer for the interception layer as well
            // as vte. Otherwise an OSC arriving one byte at a time bypasses us.
            if bytes[i] == 0x1b && i + 1 == bytes.len() {
                self.feed(&bytes[start..i]);
                self.pending_osc = bytes[i..].to_vec();
                return;
            }
            // Queries vte doesn't carry: XTVERSION (CSI > q) and XTGETTCAP
            // (DCS + q … ST). Answered here and kept from vte.
            if bytes[i] == 0x1b && i + 1 < bytes.len() {
                if bytes[i + 1] == b'['
                    && (bytes[i + 2..].starts_with(b">q") || bytes[i + 2..].starts_with(b">0q"))
                {
                    let len = if bytes[i + 2..].starts_with(b">q") {
                        4
                    } else {
                        5
                    };
                    self.feed(&bytes[start..i]);
                    self.respond(format!("\x1bP>|nus {}\x1b\\", env!("CARGO_PKG_VERSION")));
                    i += len;
                    start = i;
                    continue;
                }
                // XTWINOPS reports vte doesn't carry: CSI 11 / 13 / 16 / 19 t.
                if bytes[i + 1] == b'[' && i + 2 < bytes.len() && bytes[i + 2].is_ascii_digit() {
                    let mut j = i + 2;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b't' {
                        let op: u32 = std::str::from_utf8(&bytes[i + 2..j])
                            .unwrap_or("")
                            .parse()
                            .unwrap_or(0);
                        if matches!(op, 11 | 13 | 16 | 19) {
                            self.feed(&bytes[start..i]);
                            self.xtwinops(op);
                            i = j + 1;
                            start = i;
                            continue;
                        }
                    }
                }
                // DECXCPR: CSI ? 6 n answers with the page too.
                if bytes[i + 1] == b'[' && bytes[i + 2..].starts_with(b"?6n") {
                    self.feed(&bytes[start..i]);
                    let row = if self.modes.contains(Modes::ORIGIN) {
                        self.cursor.row - self.scroll_top
                    } else {
                        self.cursor.row
                    };
                    self.respond(format!("\x1b[?{};{};1R", row + 1, self.cursor.col + 1));
                    i += 5;
                    start = i;
                    continue;
                }
                // XTSMGRAPHICS: CSI ? Pi ; Pa ; Pv S — colours (1) and geometry (2).
                if bytes[i + 1] == b'[' && bytes[i + 2..].starts_with(b"?") {
                    let mut j = i + 3;
                    while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'S' {
                        let params: Vec<u32> = std::str::from_utf8(&bytes[i + 3..j])
                            .unwrap_or("")
                            .split(';')
                            .map(|p| p.parse().unwrap_or(0))
                            .collect();
                        self.feed(&bytes[start..i]);
                        self.xtsmgraphics(&params);
                        i = j + 1;
                        start = i;
                        continue;
                    }
                }
                // Sixel: DCS P1;P2;P3 q … ST.
                if bytes[i + 1] == b'P' {
                    let mut j = i + 2;
                    while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                        j += 1;
                    }
                    if j < bytes.len()
                        && bytes[j] == b'q'
                        && j > i + 1
                        && (j == i + 2 || bytes[i + 2].is_ascii_digit() || bytes[i + 2] == b';')
                    {
                        let params: Vec<u32> = std::str::from_utf8(&bytes[i + 2..j])
                            .unwrap_or("")
                            .split(';')
                            .map(|p| p.parse().unwrap_or(0))
                            .collect();
                        let body = j + 1;
                        let mut k = body;
                        let mut end = None;
                        while k < bytes.len() {
                            if bytes[k] == 0x9c
                                || (bytes[k] == 0x1b
                                    && k + 1 < bytes.len()
                                    && bytes[k + 1] == b'\\')
                            {
                                end = Some((k, if bytes[k] == 0x9c { k + 1 } else { k + 2 }));
                                break;
                            }
                            k += 1;
                        }
                        match end {
                            Some((e, after)) => {
                                self.feed(&bytes[start..i]);
                                self.sixel(&params, &bytes[body..e]);
                                i = after;
                                start = i;
                                continue;
                            }
                            None if bytes.len() - i < 64 * 1024 * 1024 => {
                                // Sixels can be large; wait for the rest.
                                self.feed(&bytes[start..i]);
                                self.pending_osc = bytes[i..].to_vec();
                                return;
                            }
                            None => {}
                        }
                    }
                }
                if bytes[i + 1] == b'P'
                    && (bytes[i + 2..].starts_with(b"+q") || bytes[i + 2..].starts_with(b"$q"))
                {
                    // XTGETTCAP or DECRQSS. The payload runs to ST; if it
                    // isn't here yet, wait for more.
                    let rqss = bytes[i + 2] == b'$';
                    let body = i + 4;
                    let mut j = body;
                    let mut end = None;
                    while j + 1 < bytes.len() {
                        if bytes[j] == 0x1b && bytes[j + 1] == b'\\' {
                            end = Some(j);
                            break;
                        }
                        j += 1;
                    }
                    match end {
                        Some(e) => {
                            self.feed(&bytes[start..i]);
                            let payload = bytes[body..e].to_vec();
                            if rqss {
                                self.decrqss(&payload);
                            } else {
                                self.xtgettcap(&payload);
                            }
                            i = e + 2;
                            start = i;
                            continue;
                        }
                        None if bytes.len() - i < 512 => {
                            self.feed(&bytes[start..i]);
                            self.pending_osc = bytes[i..].to_vec();
                            return;
                        }
                        None => {}
                    }
                }
            }
            // ESC ] or C1 OSC; ESC _ APC (Kitty graphics).
            let apc = bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'_';
            let osc_at = if bytes[i] == 0x1b && i + 1 < bytes.len() && (bytes[i + 1] == b']' || apc)
            {
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
            // Inspect every OSC before handing it to vte: its std parser has
            // an unbounded OSC buffer. Unhandled, bounded sequences pass through.
            let rest = &bytes[body..];
            let ours = if apc {
                rest.starts_with(b"G")
            } else {
                rest.starts_with(b"133;")
                    || rest.starts_with(b"7;")
                    || rest.starts_with(b"9;4;")
                    || rest.starts_with(b"1337;File=")
            };
            if apc && !ours && !rest.is_empty() {
                i += 1;
                continue;
            }
            // Find the terminator: BEL, ESC \, or C1 ST.
            let mut end = None;
            let mut j = body;
            let mut payload_utf8 = Utf8Prefix::default();
            while j < bytes.len() {
                // C1 ST can also occur inside a UTF-8 pathname/title. Buffered
                // control strings are rescanned from their start on each read.
                if payload_utf8.observe(bytes[j]) {
                    j += 1;
                    continue;
                }
                match bytes[j] {
                    0x07 | 0x9c => {
                        end = Some((j, j + 1));
                        break;
                    }
                    0x1b if j + 1 < bytes.len() && bytes[j + 1] == b'\\' => {
                        end = Some((j, j + 2));
                        break;
                    }
                    0x1b if j + 1 == bytes.len() => {
                        // The second byte of ESC \ may arrive in the next read.
                        j = bytes.len();
                        break;
                    }
                    0x1b => break, // another sequence began: not an OSC for us
                    _ => {}
                }
                j += 1;
            }
            let Some((pay_end, seq_end)) = end else {
                if j >= bytes.len() && (!apc || ours || rest.len() < 10) {
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
                if apc {
                    self.graphics_apc(&payload);
                } else if payload.starts_with(b"1337;File=") {
                    self.iterm_image(&payload[b"1337;File=".len()..]);
                } else {
                    self.integration_osc(&payload);
                }
                // Image payloads never reach vte; the rest does.
                start = if apc || payload.starts_with(b"1337;") {
                    seq_end
                } else {
                    at
                };
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
        // vte 0.15's partial-UTF-8 lookahead can consume a following ASCII byte
        // without dispatching it. Finish only the outstanding scalar one byte
        // at a time, then keep the normal bulk parser fast path for the rest.
        // No bytes are decoded, replaced, coalesced, or heap-buffered here.
        let mut prefix = self.feed_utf8;
        let scan_utf8 = self.scan_utf8;
        let mut offset = 0;
        while prefix.is_pending() && offset < bytes.len() {
            processor.advance(self, &bytes[offset..offset + 1]);
            prefix.observe(bytes[offset]);
            offset += 1;
        }
        if offset < bytes.len() {
            let rest = &bytes[offset..];
            processor.advance(self, rest);
            prefix = Utf8Prefix::at_end(rest);
        }
        self.processor = processor;
        // A terminal reset handled inside advance() must not erase the byte
        // boundary of the transport stream currently being processed.
        self.feed_utf8 = prefix;
        self.scan_utf8 = scan_utf8;
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
            if self.marks.len() >= 65_536 {
                self.marks.drain(..32_768);
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
            let path = url
                .strip_prefix("file://")
                .map(|u| {
                    u.split_once('/')
                        .map(|x| format!("/{}", x.1))
                        .unwrap_or_default()
                })
                .unwrap_or_else(|| url.to_string());
            let decoded = percent_decode(&path);
            let path = decoded
                .strip_prefix('/')
                .filter(|p| p.len() > 1 && p.as_bytes()[1] == b':')
                .map(|p| p.to_string())
                .unwrap_or(decoded);
            if !path.is_empty() {
                self.cwd = Some(path.clone());
                self.events.push(Event::Cwd(path));
            }
        } else if let Some(rest) = text.strip_prefix("9;4;") {
            let mut parts = rest.split(';');
            let state: u8 = parts
                .next()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            let pct: u8 = parts
                .next()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            self.progress = if state == 0 {
                None
            } else {
                Some((state, pct.min(100)))
            };
            self.events.push(Event::Progress(state, pct.min(100)));
        }
    }

    /// A sixel picture at the cursor, as an image placement.
    fn sixel(&mut self, params: &[u32], data: &[u8]) {
        let Some(pic) = crate::sixel::decode(params, data) else {
            return;
        };
        self.next_image_id += 1;
        let id = self.next_image_id;
        self.images.push(crate::images::Image {
            id,
            width: pic.width,
            height: pic.height,
            rgba: pic.rgba,
        });
        self.trim_images();
        // Sixel scrolling mode (DECSDM off, the default): the cursor ends
        // on the line after the picture.
        self.place_image(id, 0, 0, false);
    }

    /// XTSMGRAPHICS (CSI ? Pi ; Pa ; Pv S): read the colour register count
    /// (1) or the sixel geometry (2); set requests answer with what is.
    fn xtsmgraphics(&mut self, params: &[u32]) {
        let item = params.first().copied().unwrap_or(0);
        match item {
            1 => self.respond("\x1b[?1;0;256S"),
            2 => {
                let w = self.cell_px.0.max(1) as usize * self.primary.cols();
                let h = self.cell_px.1.max(1) as usize * self.primary.rows();
                self.respond(format!("\x1b[?2;0;{w};{h}S"));
            }
            _ => self.respond(format!("\x1b[?{item};1S")),
        }
    }

    /// The XTWINOPS reports beyond 14/18: window state (11), position
    /// (13), cell size (16), screen size in characters (19).
    fn xtwinops(&mut self, op: u32) {
        match op {
            11 => self.respond(b"\x1b[1t"),
            13 => self.respond(b"\x1b[3;0;0t"),
            16 => self.respond(format!(
                "\x1b[6;{};{}t",
                self.cell_px.1.max(1),
                self.cell_px.0.max(1)
            )),
            19 => self.respond(format!("\x1b[9;{};{}t", self.rows(), self.cols())),
            _ => {}
        }
    }

    /// DECRQSS (DCS $ q Pt ST): report a setting as the sequence that
    /// would set it — SGR, DECSTBM, DECSCUSR, DECSCL, DECSCA. Valid
    /// answers are `DCS 1 $ r … ST` (xterm's reading), the rest `DCS 0 $ r ST`.
    fn decrqss(&mut self, what: &[u8]) {
        let value = match what {
            b"m" => Some(format!("{}m", self.sgr_params())),
            b"r" => Some(format!(
                "{};{}r",
                self.scroll_top + 1,
                self.scroll_bottom + 1
            )),
            b" q" => {
                let s = self.cursor_style;
                let n = match (s.shape, s.blinking) {
                    (CursorShape::Block, true) => 1,
                    (CursorShape::Block, false) => 2,
                    (CursorShape::Underline, true) => 3,
                    (CursorShape::Underline, false) => 4,
                    (CursorShape::Beam, true) => 5,
                    (CursorShape::Beam, false) => 6,
                    _ => 1,
                };
                Some(format!("{n} q"))
            }
            b"\"p" => Some("64;1\"p".to_string()),
            b"\"q" => Some("0\"q".to_string()),
            _ => None,
        };
        match value {
            Some(v) => self.respond(format!("\x1bP1$r{v}\x1b\\")),
            None => self.respond(b"\x1bP0$r\x1b\\"),
        }
    }

    /// The current attributes as SGR parameters (`0;1;38:2::r:g:b…`).
    fn sgr_params(&self) -> String {
        use crate::cell::{Color, Flags};
        let t = &self.cursor.template;
        let mut p = vec!["0".to_string()];
        let f = t.flags;
        for (flag, n) in [
            (Flags::BOLD, "1"),
            (Flags::DIM, "2"),
            (Flags::ITALIC, "3"),
            (Flags::UNDERLINE, "4"),
            (Flags::DOUBLE_UL, "4:2"),
            (Flags::UNDERCURL, "4:3"),
            (Flags::DOTTED_UL, "4:4"),
            (Flags::DASHED_UL, "4:5"),
            (Flags::BLINK, "5"),
            (Flags::INVERSE, "7"),
            (Flags::HIDDEN, "8"),
            (Flags::STRIKE, "9"),
        ] {
            if f.contains(flag) {
                p.push(n.to_string());
            }
        }
        let color = |base: u8, c: Color| -> Option<String> {
            match c {
                Color::Default => None,
                Color::Indexed(i) if i < 8 && base == 30 => Some((30 + i).to_string()),
                Color::Indexed(i) if i < 8 && base == 40 => Some((40 + i).to_string()),
                Color::Indexed(i) if i < 16 && base == 30 => Some((90 + i - 8).to_string()),
                Color::Indexed(i) if i < 16 && base == 40 => Some((100 + i - 8).to_string()),
                Color::Indexed(i) => Some(format!("{}:5:{i}", base + 8)),
                Color::Rgb(r, g, b) => Some(format!("{}:2::{r}:{g}:{b}", base + 8)),
            }
        };
        p.extend(color(30, t.fg));
        p.extend(color(40, t.bg));
        if let Some(ul) = t.ul {
            p.extend(color(50, ul));
        }
        p.join(";")
    }

    /// XTGETTCAP: hex-encoded capability names, `;`-separated. Known ones
    /// come back as `DCS 1 + r name=value ST`, the rest as `DCS 0 + r ST`.
    fn xtgettcap(&mut self, payload: &[u8]) {
        fn unhex(s: &[u8]) -> Option<String> {
            let mut out = Vec::with_capacity(s.len() / 2);
            for pair in s.chunks(2) {
                let h = std::str::from_utf8(pair).ok()?;
                out.push(u8::from_str_radix(h, 16).ok()?);
            }
            String::from_utf8(out).ok()
        }
        fn hex(s: &str) -> String {
            s.bytes().map(|b| format!("{b:02X}")).collect()
        }
        for name in payload.split(|&b| b == b';') {
            let Some(cap) = unhex(name) else { continue };
            let value: Option<String> = match cap.as_str() {
                "TN" | "name" => Some("xterm-256color".into()),
                "RGB" | "Tc" => Some(String::new()),
                "colors" | "Co" => Some("256".into()),
                "setrgbf" => Some("\x1b[38:2:%p1%d:%p2%d:%p3%dm".into()),
                "setrgbb" => Some("\x1b[48:2:%p1%d:%p2%d:%p3%dm".into()),
                "Ms" => Some("\x1b]52;%p1%s;%p2%s\x1b\\".into()),
                "Ss" => Some("\x1b[%p1%d q".into()),
                "Se" => Some("\x1b[2 q".into()),
                "Smulx" => Some("\x1b[4:%p1%dm".into()),
                "bce" | "km" | "npc" => Some(String::new()),
                _ => None,
            };
            match value {
                Some(v) if v.is_empty() => self.respond(format!("\x1bP1+r{}\x1b\\", hex(&cap))),
                Some(v) => self.respond(format!("\x1bP1+r{}={}\x1b\\", hex(&cap), hex(&v))),
                None => self.respond(format!("\x1bP0+r{}\x1b\\", hex(&cap))),
            }
        }
    }

    /// Kitty graphics: `G<controls>;<base64>`.
    fn graphics_apc(&mut self, payload: &[u8]) {
        use crate::images::{control_num, control_str, kitty_controls, Pending};
        let payload = &payload[1..];
        let (ctl, data) = match payload.iter().position(|&b| b == b';') {
            Some(p) => (&payload[..p], &payload[p + 1..]),
            None => (payload, &payload[..0]),
        };
        let c = kitty_controls(&String::from_utf8_lossy(ctl));
        let action = control_str(&c, 'a').unwrap_or("t").to_string();
        let quiet = control_num(&c, 'q', 0) as u8;
        let id = control_num(&c, 'i', 0) as u32;
        let more = control_num(&c, 'm', 0) == 1;
        let respond = |me: &mut Term, id: u32, msg: &str| {
            if (quiet == 0 || (quiet == 1 && msg != "OK")) && id != 0 {
                me.responses
                    .extend_from_slice(format!("\x1b_Gi={id};{msg}\x1b\\").as_bytes());
            }
        };
        match action.as_str() {
            "q" => {
                respond(self, id, "OK");
            }
            "d" => {
                let what = control_str(&c, 'd').unwrap_or("a");
                match what {
                    "i" | "I" => {
                        let target = control_num(&c, 'i', 0) as u32;
                        self.placements.retain(|p| p.image != target);
                        if what == "I" {
                            self.images.retain(|im| im.id != target);
                        }
                    }
                    _ => self.placements.clear(),
                }
                self.images_gen += 1;
            }
            "p" => {
                if self.images.iter().any(|im| im.id == id) {
                    self.place_image(
                        id,
                        control_num(&c, 'c', 0) as usize,
                        control_num(&c, 'r', 0) as usize,
                        control_num(&c, 'C', 0) == 1,
                    );
                    respond(self, id, "OK");
                } else {
                    respond(self, id, "ENOENT:no image with that id");
                }
            }
            _ => {
                // t / T: transmit (and display). Chunks accumulate until m=0.
                let mut pending = self.pending_image.take().unwrap_or_else(|| Pending {
                    id,
                    format: control_num(&c, 'f', 32) as u32,
                    width: control_num(&c, 's', 0) as u32,
                    height: control_num(&c, 'v', 0) as u32,
                    data: Vec::new(),
                    display: action == "T",
                    cols: control_num(&c, 'c', 0) as usize,
                    rows: control_num(&c, 'r', 0) as usize,
                    quiet,
                });
                if data.len() > crate::images::MAX_ENCODED_BYTES.saturating_sub(pending.data.len())
                {
                    respond(self, pending.id, "E2BIG:image transmission limit exceeded");
                    return;
                }
                pending.data.extend_from_slice(data);
                if more {
                    self.pending_image = Some(pending);
                    return;
                }
                let bytes = crate::images::base64_decode(&pending.data);
                let decoded = match pending.format {
                    100 => crate::images::decode_png(&bytes),
                    f => crate::images::decode_raw(f, pending.width, pending.height, &bytes)
                        .map(|rgba| (pending.width, pending.height, rgba)),
                };
                let Some((w, h, rgba)) = decoded else {
                    respond(self, pending.id, "EINVAL:could not decode");
                    return;
                };
                let id = if pending.id == 0 {
                    self.next_image_id += 1;
                    self.next_image_id
                } else {
                    pending.id
                };
                self.images.retain(|im| im.id != id);
                self.images.push(crate::images::Image {
                    id,
                    width: w,
                    height: h,
                    rgba,
                });
                self.trim_images();
                if pending.display {
                    self.place_image(id, pending.cols, pending.rows, false);
                }
                self.images_gen += 1;
                respond(self, pending.id, "OK");
            }
        }
    }

    /// Keep image memory under 64 MB: oldest first, placements with them.
    fn trim_images(&mut self) {
        let mut total: usize = self.images.iter().map(|im| im.rgba.len()).sum();
        while (total > crate::images::MAX_RGBA_BYTES || self.images.len() > 1024)
            && !self.images.is_empty()
        {
            let gone = self.images.remove(0);
            total -= gone.rgba.len();
            self.placements.retain(|p| p.image != gone.id);
        }
    }

    /// Put an image at the cursor and move past it (unless `keep_cursor`).
    fn place_image(&mut self, id: u32, cols: usize, rows: usize, keep_cursor: bool) {
        let Some(im) = self.images.iter().find(|im| im.id == id) else {
            return;
        };
        let (cw, ch) = (self.cell_px.0.max(1) as f32, self.cell_px.1.max(1) as f32);
        let cols = if cols > 0 {
            cols
        } else {
            (im.width as f32 / cw).ceil().max(1.0) as usize
        };
        let rows = if rows > 0 {
            rows
        } else {
            (im.height as f32 / ch).ceil().max(1.0) as usize
        };
        let rows = rows.min(16_384);
        let cols = cols.min(self.primary.cols().max(1));
        let line = self.primary.abs_row(self.cursor.row);
        let col = self.cursor.col;
        if self.placements.len() >= 4096 {
            self.placements.drain(..2048);
        }
        self.placements.push(crate::images::Placement {
            image: id,
            line,
            col,
            cols,
            rows,
        });
        self.images_gen += 1;
        if !keep_cursor {
            for _ in 1..rows {
                Handler::linefeed(self);
            }
            self.cursor.col = (col + cols).min(self.primary.cols().saturating_sub(1));
            self.cursor.wrap_next = false;
        }
    }

    /// iTerm2 inline image: `<args>:<base64>`.
    fn iterm_image(&mut self, payload: &[u8]) {
        use crate::images::{iterm_args, iterm_size};
        let Some(colon) = payload.iter().position(|&b| b == b':') else {
            return;
        };
        let args = iterm_args(&String::from_utf8_lossy(&payload[..colon]));
        let inline = args.iter().any(|(k, v)| k == "inline" && v == "1");
        if !inline {
            return;
        }
        let bytes = crate::images::base64_decode(&payload[colon + 1..]);
        let Some((w, h, rgba)) = crate::images::decode_png(&bytes) else {
            return;
        };
        self.next_image_id += 1;
        let id = self.next_image_id;
        self.images.push(crate::images::Image {
            id,
            width: w,
            height: h,
            rgba,
        });
        self.trim_images();
        let width = args
            .iter()
            .find(|(k, _)| k == "width")
            .map(|(_, v)| v.as_str());
        let height = args
            .iter()
            .find(|(k, _)| k == "height")
            .map(|(_, v)| v.as_str());
        let cols = iterm_size(width, w, self.cell_px.0, self.primary.cols()).unwrap_or(0);
        let rows = iterm_size(height, h, self.cell_px.1, self.primary.rows()).unwrap_or(0);
        // Keep the aspect when only one side is given.
        let (cols, rows) = match (cols, rows) {
            (0, 0) => (0, 0),
            (c, 0) => (
                c,
                ((c as f32 * self.cell_px.0 as f32) * h as f32
                    / w as f32
                    / self.cell_px.1.max(1) as f32)
                    .ceil()
                    .max(1.0) as usize,
            ),
            (0, r) => (
                ((r as f32 * self.cell_px.1 as f32) * w as f32
                    / h as f32
                    / self.cell_px.0.max(1) as f32)
                    .ceil()
                    .max(1.0) as usize,
                r,
            ),
            (c, r) => (c, r),
        };
        self.place_image(id, cols, rows, false);
        self.images_gen += 1;
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

    /// Text between two absolute positions (inclusive), joining wrapped rows
    /// without a newline. Positions are (line, col); order doesn't matter.
    pub fn text_range(&self, a: (u64, usize), b: (u64, usize)) -> String {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let grid = &self.primary;
        let mut out = String::new();
        let mut line = start.0;
        while line <= end.0 {
            let Some(row) = grid.row_abs(line) else {
                line += 1;
                continue;
            };
            let chars: Vec<char> = row
                .cells
                .iter()
                .filter(|c| !c.flags.contains(crate::cell::Flags::WIDE_SPACER))
                .map(|c| c.ch)
                .collect();
            let from = if line == start.0 { start.1 } else { 0 };
            let to = if line == end.0 {
                (end.1 + 1).min(chars.len())
            } else {
                chars.len()
            };
            let piece: String = chars
                .get(from.min(chars.len())..to.max(from.min(chars.len())))
                .unwrap_or(&[])
                .iter()
                .collect();
            if line == end.0 || row.wrapped {
                out.push_str(&piece);
            } else {
                out.push_str(piece.trim_end());
                out.push('\n');
            }
            line += 1;
        }
        out
    }

    /// The word around a cell: letters, digits and path-ish punctuation.
    pub fn word_at(&self, line: u64, col: usize) -> Option<(usize, usize)> {
        let row = self.primary.row_abs(line)?;
        let is_word = |c: char| c.is_alphanumeric() || "_-./\\:~@+=%".contains(c);
        let chars: Vec<char> = row.cells.iter().map(|c| c.ch).collect();
        let c = chars.get(col).copied()?;
        if !is_word(c) {
            return Some((col, col));
        }
        let mut a = col;
        while a > 0 && is_word(chars[a - 1]) {
            a -= 1;
        }
        let mut b = col;
        while b + 1 < chars.len() && is_word(chars[b + 1]) {
            b += 1;
        }
        Some((a, b))
    }

    /// The last non-blank column of a line (for line selection).
    pub fn line_end(&self, line: u64) -> usize {
        self.primary
            .row_abs(line)
            .map(|r| r.text().chars().count().saturating_sub(1))
            .unwrap_or(0)
    }

    /// Case-insensitive matches of `q` in history and the screen: (line, col, len).
    pub fn search(&self, q: &str) -> Vec<(u64, usize, usize)> {
        let q: Vec<char> = q.to_lowercase().chars().collect();
        if q.is_empty() {
            return Vec::new();
        }
        let grid = &self.primary;
        let mut out = Vec::new();
        let mut line = grid.oldest_abs();
        let last = grid.abs_row(grid.rows() - 1);
        while line <= last {
            if let Some(row) = grid.row_abs(line) {
                let chars: Vec<char> = row
                    .cells
                    .iter()
                    .map(|c| c.ch.to_lowercase().next().unwrap_or(c.ch))
                    .collect();
                if chars.len() >= q.len() {
                    let mut i = 0;
                    while i + q.len() <= chars.len() {
                        if chars[i..i + q.len()] == q[..] {
                            out.push((line, i, q.len()));
                            i += q.len();
                        } else {
                            i += 1;
                        }
                    }
                }
            }
            line += 1;
        }
        out
    }

    /// The block a line belongs to: (prompt line, command text, exit) from marks.
    pub fn block_at(&self, line: u64) -> Option<(u64, u64, String, Option<i32>)> {
        let starts: Vec<usize> = self
            .marks
            .iter()
            .enumerate()
            .filter(|(_, m)| m.kind == MarkKind::PromptStart)
            .map(|(i, _)| i)
            .collect();
        let idx = starts.iter().rposition(|&i| self.marks[i].line <= line)?;
        let start = self.marks[starts[idx]].line;
        let end = starts
            .get(idx + 1)
            .map(|&i| self.marks[i].line)
            .unwrap_or(self.primary.abs_row(self.primary.rows() - 1) + 1);
        let cmd = self.marks[starts[idx]..]
            .iter()
            .find(|m| m.kind == MarkKind::CommandStart)
            .map(|b| self.command_text(b))
            .unwrap_or_default();
        let exit = self.marks[starts[idx]..]
            .iter()
            .take_while(|m| m.line < end || m.kind != MarkKind::PromptStart)
            .find_map(|m| match m.kind {
                MarkKind::CommandEnd(e) => Some(e),
                _ => None,
            })
            .flatten();
        Some((start, end, cmd, exit))
    }

    /// Is the shell sitting at a prompt (the last mark is A or B)?
    pub fn at_prompt(&self) -> bool {
        matches!(
            self.marks.last().map(|m| m.kind),
            Some(MarkKind::PromptStart | MarkKind::CommandStart)
        )
    }

    /// The command text between a B mark and the next C (or the cursor).
    pub fn command_text(&self, b: &Mark) -> String {
        let grid = &self.primary;
        let end = self
            .marks
            .iter()
            .find(|m| {
                matches!(m.kind, MarkKind::OutputStart)
                    && (m.line > b.line || (m.line == b.line && m.col >= b.col))
            })
            .map(|m| (m.line, m.col))
            .unwrap_or((grid.abs_row(self.cursor.row), self.cursor.col));
        let mut out = String::new();
        let mut line = b.line;
        while line <= end.0 {
            if let Some(row) = grid.row_abs(line) {
                let text: String = row.text();
                let from = if line == b.line { b.col } else { 0 };
                let to = if line == end.0 {
                    end.1.min(text.chars().count())
                } else {
                    text.chars().count()
                };
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
            PrivateMode::Unknown(1016) => {
                self.modes.set(Modes::MOUSE_SGR_PIXEL, on);
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
            PrivateMode::Unknown(1016) => Modes::MOUSE_SGR_PIXEL,
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
            // VT220 with sixel (4) and ANSI colour (22).
            None => self.respond(b"\x1b[?62;4;22c"),
            Some('>') => self.respond(b"\x1b[>1;10;0c"),
            // DA3: a unit id; we have none, so all zeros.
            Some('=') => self.respond(b"\x1bP!|00000000\x1b\\"),
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
                // IDs already stored in cells must never be recycled. Stop
                // accepting new links at the budget; ordinary text still prints.
                if self.hyperlinks.last() == Some(&h.uri) {
                    self.hyperlinks.len() as u32
                } else if self.hyperlinks.len() >= 16_384
                    || h.uri.len() > (4 * 1024 * 1024usize).saturating_sub(self.hyperlink_bytes)
                {
                    0
                } else {
                    self.hyperlink_bytes += h.uri.len();
                    self.hyperlinks.push(h.uri);
                    self.hyperlinks.len() as u32
                }
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

#[cfg(test)]
mod tests {

    #[test]
    fn unterminated_control_is_bounded_and_recovers_after_split_terminator() {
        let mut t = term(20, 2);
        t.advance(b"\x1b]7;file://host/");
        let chunk = [b'x'; 16 * 1024];
        for _ in 0..32 {
            t.advance(&chunk);
        }
        assert!(t.discard_control);
        assert!(t.pending_osc.is_empty());
        // U+271C includes a UTF-8 continuation byte equal to C1 ST.
        t.advance("✜".as_bytes());
        assert!(t.discard_control);
        t.advance(b"\x1b");
        t.advance(b"\\ok");
        assert!(!t.discard_control);
        assert_eq!(t.grid().text().trim(), "ok");
        t.advance(b"\x1b]7;file://host/recovered\x07");
        assert_eq!(t.cwd.as_deref(), Some("/recovered"));
    }

    #[test]
    fn unhandled_osc_cannot_grow_the_underlying_vte_parser() {
        for prefix in [b"\x1b]0;".as_slice(), b"\x1b]999;"] {
            let mut t = term(20, 2);
            t.advance(prefix);
            for _ in 0..20 {
                t.advance(&[b'x'; 16 * 1024]);
            }
            assert!(t.discard_control);
            assert!(t.pending_osc.is_empty());
            t.advance(b"\x07ok");
            assert_eq!(t.grid().text().trim(), "ok");
        }
    }

    #[test]
    fn kitty_chunk_budget_rejects_and_releases_accumulated_data() {
        let mut t = term(20, 2);
        t.pending_image = Some(crate::images::Pending {
            id: 42,
            data: vec![b'A'; crate::images::MAX_ENCODED_BYTES],
            ..Default::default()
        });
        t.graphics_apc(b"Gm=1;AAAA");
        assert!(t.pending_image.is_none());
        assert!(String::from_utf8(t.take_responses())
            .unwrap()
            .contains("E2BIG"));
        assert!(t.images.is_empty());
    }

    #[test]
    fn repeated_sixels_and_placements_obey_retention_budgets() {
        let mut t = term(20, 2);
        for _ in 0..1100 {
            t.sixel(&[], b"~");
        }
        assert_eq!(t.images.len(), 1024);
        assert!(t
            .placements
            .iter()
            .all(|p| t.images.iter().any(|i| i.id == p.image)));
        let id = t.images.last().unwrap().id;
        for _ in 0..5000 {
            t.place_image(id, 1, 1, true);
        }
        assert!(t.placements.len() <= 4096);
    }

    #[test]
    fn hyperlink_budget_preserves_old_cell_ids() {
        let mut t = term(20, 2);
        t.advance(b"\x1b]8;;https://example.com/first\x07a\x1b]8;;\x07");
        for i in 0..16_500 {
            t.advance(format!("\x1b]8;;https://example.com/{i}\x07").as_bytes());
        }
        assert_eq!(t.hyperlinks.len(), 16_384);
        assert_eq!(t.hyperlinks[0], "https://example.com/first");
        assert_eq!(t.cursor.template.link, 0);
    }

    #[test]
    fn a_sixel_becomes_a_placement() {
        let mut t = Term::new(80, 24, 100);
        t.advance(b"\x1bP0;1q#1;2;100;0;0!8~-!8~\x1b\\after");
        assert_eq!(t.images.len(), 1);
        assert_eq!((t.images[0].width, t.images[0].height), (8, 12));
        assert_eq!(t.placements.len(), 1);
        // Split across chunks, too.
        t.advance(b"\x1bP0;1q#2;2;0;0;100");
        t.advance(b"~~~~\x1b\\");
        assert_eq!(t.images.len(), 2);
        // The geometry query answers in pixels.
        t.take_responses();
        t.advance(b"\x1b[?2;1;0S");
        let r = String::from_utf8(t.take_responses()).unwrap();
        assert!(r.starts_with("\x1b[?2;0;"), "{r:?}");
    }

    #[test]
    fn xtversion_and_xtgettcap_answer() {
        let mut t = Term::new(80, 24, 0);
        t.advance(b"\x1b[>q");
        let r = String::from_utf8(t.take_responses()).unwrap();
        assert!(r.starts_with("\x1bP>|nus "), "{r:?}");
        // "TN" and "Tc" and an unknown "zz", hex-encoded, one DCS.
        t.advance(b"\x1bP+q544e;5463;7a7a\x1b\\");
        let r = String::from_utf8(t.take_responses()).unwrap();
        assert!(
            r.contains("\x1bP1+r544E=")
                && r.contains("\x1bP1+r5463\x1b\\")
                && r.contains("\x1bP0+r7A7A"),
            "{r:?}"
        );
        // Split across chunks.
        t.advance(b"\x1bP+q5247");
        t.advance(b"42\x1b\\");
        let r = String::from_utf8(t.take_responses()).unwrap();
        assert!(r.contains("1+r524742"), "{r:?}");
    }
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
        feed(
            &mut t,
            "\x1b]133;A\x07$ \x1b]133;B\x07ls -la\r\n\x1b]133;C\x07a\r\nb\r\nc\r\n\x1b]133;D;0\x07",
        );
        let kinds: Vec<MarkKind> = t.marks.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            vec![
                MarkKind::PromptStart,
                MarkKind::CommandStart,
                MarkKind::OutputStart,
                MarkKind::CommandEnd(Some(0))
            ]
        );
        // The prompt was on absolute line 0, which has scrolled into history.
        assert_eq!(t.marks[0].line, 0);
        assert_eq!(t.marks[1].col, 2);
        assert!(matches!(
            t.grid().locate(0),
            Some(crate::grid::Loc::History(_))
        ));
        assert_eq!(t.command_text(&t.marks[1].clone()), "ls -la");
        assert_eq!(
            t.grid().row_abs(0).map(|r| r.text().trim_end().to_string()),
            Some("$ ls -la".into())
        );
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
    fn kitty_raw_rgba_image_places_and_moves_the_cursor() {
        let mut t = term(20, 6);
        t.cell_px = (8, 16);
        // 2×2 RGBA, chunked over two APCs; 16px wide → 2 cols, 32px → 2 rows when sized in px.
        let px: Vec<u8> = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let b64 = {
            const T: &[u8; 64] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut out = String::new();
            for chunk in px.chunks(3) {
                let b = [
                    chunk[0],
                    *chunk.get(1).unwrap_or(&0),
                    *chunk.get(2).unwrap_or(&0),
                ];
                let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
                out.push(T[(n >> 18) as usize & 63] as char);
                out.push(T[(n >> 12) as usize & 63] as char);
                out.push(if chunk.len() > 1 {
                    T[(n >> 6) as usize & 63] as char
                } else {
                    '='
                });
                out.push(if chunk.len() > 2 {
                    T[n as usize & 63] as char
                } else {
                    '='
                });
            }
            out
        };
        let (a, b) = b64.split_at(8);
        feed(
            &mut t,
            &format!("x\x1b_Ga=T,f=32,s=2,v=2,i=3,c=4,r=2,m=1;{a}\x1b\\"),
        );
        assert!(t.images.is_empty());
        feed(&mut t, &format!("\x1b_Gm=0;{b}\x1b\\y"));
        assert_eq!(t.images.len(), 1);
        assert_eq!(t.images[0].rgba, px);
        assert_eq!(t.placements.len(), 1);
        let pl = t.placements[0];
        assert_eq!((pl.line, pl.col, pl.cols, pl.rows), (0, 1, 4, 2));
        // Cursor moved past the image: row 1, col 5, and "y" landed there.
        assert_eq!(t.grid().row(1).text().trim_end(), "     y");
        let resp = String::from_utf8_lossy(&t.take_responses()).to_string();
        assert!(resp.contains("Gi=3;OK"), "{resp:?}");
        // Delete all placements keeps the image.
        feed(&mut t, "\x1b_Ga=d\x1b\\");
        assert!(t.placements.is_empty());
        assert_eq!(t.images.len(), 1);
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
        assert_eq!(t.take_responses(), b"\x1b[3;4R\x1b[?62;4;22c");
        feed(&mut t, "\x1b[?6n\x1b[=c");
        assert_eq!(t.take_responses(), b"\x1b[?3;4;1R\x1bP!|00000000\x1b\\");
    }

    #[test]
    fn xtwinops_reports_and_decrqss() {
        let mut t = term(10, 5);
        t.cell_px = (8, 16);
        feed(&mut t, "\x1b[16t\x1b[19t\x1b[11t");
        assert_eq!(t.take_responses(), b"\x1b[6;16;8t\x1b[9;5;10t\x1b[1t");
        feed(
            &mut t,
            "\x1b[1;4m\x1b[38;2;1;2;3m\x1b[2;4r\x1bP$qm\x1b\\\x1bP$qr\x1b\\\x1bP$qx\x1b\\",
        );
        assert_eq!(
            String::from_utf8(t.take_responses()).unwrap(),
            "\x1bP1$r0;1;4;38:2::1:2:3m\x1b\\\x1bP1$r2;4r\x1b\\\x1bP0$r\x1b\\"
        );
        feed(&mut t, "\x1b[?1016h\x1b[?1016$p");
        assert_eq!(t.take_responses(), b"\x1b[?1016;1$y");
        assert!(t.modes().contains(Modes::MOUSE_SGR_PIXEL));
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
