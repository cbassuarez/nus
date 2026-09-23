// Exercise the benchmark's actual fixtures and scroll adapter in ordinary CI.
#[path = "../benches/support/mod.rs"]
mod support;

use nus_vt::{Grid, Term};
use support::{fixture, populated_term, scroll_full_screen, validate, TerminalFixture};

#[test]
fn full_screen_scroll_uses_inclusive_last_row() {
    for rows in [1usize, 24, 40] {
        for n in [0usize, 1, 10, 40, 100] {
            let mut grid = Grid::new(8, rows, 100);
            for row in 0..rows {
                grid.cell_mut(row, 0).ch = char::from_u32('A' as u32 + row as u32).unwrap();
            }
            let before: Vec<_> = (0..rows).map(|row| grid.row(row).cells.clone()).collect();
            scroll_full_screen(&mut grid, n);
            let moved = n.min(rows);
            assert_eq!(grid.rows(), rows);
            assert_eq!(grid.scrollback_len(), moved);
            for row in 0..rows - moved {
                assert_eq!(grid.row(row).cells, before[row + moved]);
            }
            for row in rows - moved..rows {
                assert_eq!(grid.row(row).text(), "");
            }
        }
    }
}

#[test]
fn benchmark_scroll_retains_and_bounds_history() {
    let base = populated_term(120, 40, 10_000, 4_000);
    for n in [1usize, 10, 40] {
        let mut grid = base.grid().clone();
        let history = grid.scrollback_len();
        scroll_full_screen(&mut grid, n);
        assert_eq!(grid.scrollback_len(), history + n);
        assert_eq!(grid.rows(), 40);
    }
    let mut limited = Grid::new(8, 40, 3);
    for _ in 0..5 {
        scroll_full_screen(&mut limited, 40);
        assert_eq!(limited.scrollback_len(), 3);
    }
}

fn assert_same_state(expected: &Term, actual: &Term, context: &str) {
    assert_eq!(expected.cols(), actual.cols(), "{context}");
    assert_eq!(expected.rows(), actual.rows(), "{context}");
    assert_eq!(expected.cursor().row, actual.cursor().row, "{context}");
    assert_eq!(expected.cursor().col, actual.cursor().col, "{context}");
    assert_eq!(
        expected.cursor().wrap_next,
        actual.cursor().wrap_next,
        "{context}: pending wrap"
    );
    assert_eq!(
        expected.cursor().template,
        actual.cursor().template,
        "{context}: attributes"
    );
    assert_eq!(expected.modes(), actual.modes(), "{context}: modes");
    let a = expected.grid();
    let b = actual.grid();
    assert_eq!(a.scrollback_len(), b.scrollback_len(), "{context}");
    let start = a.abs_of_display(0) - a.scrollback_len() as u64;
    assert_eq!(a.abs_of_display(0), b.abs_of_display(0), "{context}");
    for abs in start..a.abs_of_display(0) + a.rows() as u64 {
        let (left, right) = (a.row_abs(abs).unwrap(), b.row_abs(abs).unwrap());
        assert_eq!(left.cells, right.cells, "{context}: absolute row {abs}");
        assert_eq!(left.wrapped, right.wrapped, "{context}: absolute row {abs}");
    }
}

#[test]
fn fixture_delivery_chunks_preserve_terminal_state() {
    for case in [
        TerminalFixture::PlainAscii { bytes: 8 * 1024 },
        TerminalFixture::AnsiLog { lines: 100 },
        TerminalFixture::CursorHeavy { frames: 40 },
        TerminalFixture::TuiRedraw {
            rows: 40,
            cols: 120,
            frames: 3,
        },
        TerminalFixture::CombiningUnicode { graphemes: 128 },
        TerminalFixture::WideUnicode { cells: 128 },
        TerminalFixture::ScrollStream { lines: 200 },
    ] {
        let bytes = fixture(case);
        validate(case, &bytes);
        let mut expected = Term::new(120, 40, 10_000);
        expected.advance(&bytes);
        for chunk in [1usize, 2, 3, 7, 31, 63, 64, 65, 4096] {
            let mut actual = Term::new(120, 40, 10_000);
            for part in bytes.chunks(chunk) {
                actual.advance(part);
            }
            assert_same_state(&expected, &actual, &format!("{case:?}, chunk={chunk}"));
        }
    }
}
