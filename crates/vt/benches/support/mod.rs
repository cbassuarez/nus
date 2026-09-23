use nus_vt::{Cell, Grid, Term};

#[derive(Clone, Copy, Debug)]
pub enum TerminalFixture {
    PlainAscii {
        bytes: usize,
    },
    AnsiLog {
        lines: usize,
    },
    CursorHeavy {
        frames: usize,
    },
    TuiRedraw {
        rows: usize,
        cols: usize,
        frames: usize,
    },
    CombiningUnicode {
        graphemes: usize,
    },
    WideUnicode {
        cells: usize,
    },
    ScrollStream {
        lines: usize,
    },
}

pub fn fixture(case: TerminalFixture) -> Vec<u8> {
    match case {
        TerminalFixture::PlainAscii { bytes } => repeat_to_at_least(
            b"compile src/main.rs -> target/debug/nus  finished ok\r\n",
            bytes,
        ),
        TerminalFixture::AnsiLog { lines } => {
            let mut out = Vec::with_capacity(lines * 64);
            for i in 0..lines {
                let level = match i % 4 {
                    0 => "32mINFO",
                    1 => "33mWARN",
                    2 => "36mTRACE",
                    _ => "31mERROR",
                };
                out.extend_from_slice(
                    format!(
                        "\x1b[{level}\x1b[0m worker={:02} item={:06} complete\r\n",
                        i % 16,
                        i
                    )
                    .as_bytes(),
                );
            }
            out
        }
        TerminalFixture::CursorHeavy { frames } => {
            let mut out = Vec::with_capacity(frames * 96);
            for i in 0..frames {
                out.extend_from_slice(b"\x1b[2K\r");
                out.extend_from_slice(format!("index {:05}  [", i).as_bytes());
                out.extend(std::iter::repeat_n(b'=', i % 48));
                out.extend_from_slice(b">]\x1b[1A\x1b[2K\x1b[1B");
            }
            out
        }
        TerminalFixture::TuiRedraw { rows, cols, frames } => {
            let mut out = Vec::with_capacity(rows * cols * frames);
            for frame in 0..frames {
                out.extend_from_slice(b"\x1b[H");
                for row in 0..rows {
                    out.extend_from_slice(b"\x1b[2K");
                    let prefix = format!("{:02}:{:02} ", frame % 100, row % 100);
                    out.extend_from_slice(prefix.as_bytes());
                    let body = cols.saturating_sub(prefix.len());
                    out.extend(std::iter::repeat_n(b'a' + ((row + frame) % 26) as u8, body));
                    if row + 1 != rows {
                        out.extend_from_slice(b"\r\n");
                    }
                }
            }
            out
        }
        TerminalFixture::CombiningUnicode { graphemes } => {
            "Cafe\u{301} nai\u{308}ve re\u{301}sume\u{301} — "
                .repeat(graphemes.max(1) / 4 + 1)
                .into_bytes()
        }
        TerminalFixture::WideUnicode { cells } => "界面 終端 日本語 한글 🙂🚀 "
            .repeat(cells.max(1) / 8 + 1)
            .into_bytes(),
        TerminalFixture::ScrollStream { lines } => {
            let mut out = Vec::with_capacity(lines * 64);
            for i in 0..lines {
                out.extend_from_slice(
                    format!(
                        "{:08} cargo: compiling deterministic fixture {:08}\r\n",
                        i, i
                    )
                    .as_bytes(),
                );
            }
            out
        }
    }
}

fn repeat_to_at_least(seed: &[u8], bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes + seed.len());
    while out.len() < bytes {
        out.extend_from_slice(seed);
    }
    out.truncate(bytes);
    out
}

pub fn populated_term(cols: usize, rows: usize, scrollback: usize, lines: usize) -> Term {
    let mut term = Term::new(cols, rows, scrollback);
    let bytes = fixture(TerminalFixture::ScrollStream { lines });
    term.advance(&bytes);
    term
}

pub fn validate(case: TerminalFixture, bytes: &[u8]) {
    assert!(!bytes.is_empty(), "benchmark fixture must not be empty");
    let mut term = Term::new(120, 40, 10_000);
    term.advance(bytes);
    assert_eq!(term.cols(), 120);
    assert_eq!(term.rows(), 40);
    match case {
        TerminalFixture::ScrollStream { lines } if lines > 40 => {
            assert!(
                term.grid().scrollback_len() > 0,
                "scroll fixture must exercise history"
            );
        }
        _ => {}
    }
}

/// The production API takes an INCLUSIVE bottom row, not a row count.
/// Shared by the benchmark and ordinary tests so their bounds cannot drift.
pub fn scroll_full_screen(grid: &mut Grid, lines: usize) {
    let rows = grid.rows();
    let Some(bottom) = rows.checked_sub(1) else {
        return;
    };
    grid.scroll_up(0, bottom, lines.min(rows), &Cell::default(), true);
}
