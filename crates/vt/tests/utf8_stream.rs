//! Byte-delivery regressions: the PTY may split anywhere, including within a
//! scalar. These tests use the production Term path, not a benchmark adapter.
use nus_vt::{Event, Term};
use vte::ansi::Handler;

fn assert_visible_state(expected: &Term, actual: &Term, context: &str) {
    assert_eq!(expected.cols(), actual.cols(), "{context}: columns");
    assert_eq!(expected.rows(), actual.rows(), "{context}: rows");
    assert_eq!(
        expected.cursor().row,
        actual.cursor().row,
        "{context}: cursor row"
    );
    assert_eq!(
        expected.cursor().col,
        actual.cursor().col,
        "{context}: cursor column"
    );
    assert_eq!(
        expected.cursor().wrap_next,
        actual.cursor().wrap_next,
        "{context}: wrap"
    );
    assert_eq!(
        expected.cursor().template,
        actual.cursor().template,
        "{context}: attributes"
    );
    assert_eq!(expected.modes(), actual.modes(), "{context}: modes");
    let (left, right) = (expected.grid(), actual.grid());
    assert_eq!(
        left.scrollback_len(),
        right.scrollback_len(),
        "{context}: history length"
    );
    assert_eq!(
        left.abs_of_display(0),
        right.abs_of_display(0),
        "{context}: history position"
    );
    let first = left.abs_of_display(0) - left.scrollback_len() as u64;
    for abs in first..left.abs_of_display(0) + left.rows() as u64 {
        let (a, b) = (left.row_abs(abs).unwrap(), right.row_abs(abs).unwrap());
        assert_eq!(a.cells, b.cells, "{context}: absolute row {abs}");
        assert_eq!(a.wrapped, b.wrapped, "{context}: wrapped row {abs}");
    }
}

fn text_reference(text: &str) -> Term {
    // Independent oracle for printable/combining text: bypass the byte parser,
    // but use the same character-width/cell policy as the actual terminal.
    let mut term = Term::new(24, 6, 100);
    for ch in text.chars() {
        Handler::input(&mut term, ch);
    }
    term
}

#[test]
fn split_combining_character_keeps_following_space() {
    // vte 0.15 lookahead sees CC 81 20 CC and reports three valid bytes while
    // dispatching only U+0301. The space must not disappear from Term's input.
    let mut actual = Term::new(24, 6, 100);
    actual.advance(b"A\xcc");
    actual.advance(b"\x81 \xcc\x81Z");
    assert_eq!(actual.cursor().col, 3);
    assert_eq!(actual.grid().cell(0, 1).ch, ' ');
    assert_eq!(actual.grid().cell(0, 2).ch, 'Z');
    assert_visible_state(
        &text_reference("A\u{301} \u{301}Z"),
        &actual,
        "minimal lookahead",
    );
}

#[test]
fn unicode_text_survives_every_two_part_split() {
    for text in [
        "A\u{301} \u{301}Z",
        "Cafe\u{301} nai\u{308}ve re\u{301}sume\u{301} — ",
        "面",
        "“”",
        "界面 終端 日本語 한글 🙂🚀 ",
        "\u{80}\u{9c}\u{9d}",
    ] {
        // Encoded C1 controls use vte's control policy, not Handler::input.
        let expected = if text.chars().any(|c| ('\u{80}'..='\u{9f}').contains(&c)) {
            let mut term = Term::new(24, 6, 100);
            term.advance(text.as_bytes());
            term
        } else {
            text_reference(text)
        };
        for split in 0..=text.len() {
            let mut actual = Term::new(24, 6, 100);
            actual.advance(&text.as_bytes()[..split]);
            actual.advance(&[]); // An empty read cannot flush an unfinished scalar.
            actual.advance(&text.as_bytes()[split..]);
            assert_visible_state(&expected, &actual, &format!("{text:?}, split={split}"));
        }
    }
}

#[test]
fn unicode_text_survives_fixed_and_irregular_chunks() {
    let text = "Cafe\u{301} nai\u{308}ve — 界面 本 한글 🙂🚀 ".repeat(37);
    let expected = text_reference(&text);
    for widths in [
        vec![1],
        vec![2],
        vec![3],
        vec![7],
        vec![31],
        vec![63],
        vec![64],
        vec![65],
        vec![4096],
        vec![1, 3, 64, 2, 7, 31, 5],
    ] {
        let mut actual = Term::new(24, 6, 100);
        let mut offset = 0;
        for &width in widths.iter().cycle() {
            if offset == text.len() {
                break;
            }
            let end = (offset + width).min(text.len());
            actual.advance(&text.as_bytes()[offset..end]);
            offset = end;
        }
        assert_visible_state(&expected, &actual, &format!("widths={widths:?}"));
    }
}

#[test]
fn utf8_boundaries_survive_terminal_reset() {
    let bytes = "old\x1bcA\u{301} \u{301}面Z".as_bytes();
    let mut expected = Term::new(24, 6, 100);
    expected.advance(bytes);
    for split in 0..=bytes.len() {
        let mut actual = Term::new(24, 6, 100);
        actual.advance(&bytes[..split]);
        actual.advance(&bytes[split..]);
        assert_visible_state(&expected, &actual, &format!("RIS, split={split}"));
    }
}

#[test]
fn malformed_utf8_does_not_swallow_following_ascii() {
    for bytes in [
        b"A\xe0\x80Z".as_slice(),
        b"A\xed\xa0Z",
        b"A\xf0\x80Z",
        b"A\xf4\x90Z",
        b"A\xc0\x80Z",
        b"A\xf0\x28\x8c\x28Z",
    ] {
        let mut expected = Term::new(24, 6, 100);
        expected.advance(bytes);
        for split in 0..=bytes.len() {
            let mut actual = Term::new(24, 6, 100);
            actual.advance(&bytes[..split]);
            actual.advance(&bytes[split..]);
            assert_visible_state(&expected, &actual, &format!("{bytes:?}, split={split}"));
        }
    }
}

#[test]
fn unicode_osc_path_is_not_terminated_by_continuation_bytes() {
    let path = "/面本한/“notes”";
    for terminator in [b"\x07".as_slice(), b"\x1b\\", b"\x9c"] {
        let mut bytes = format!("\x1b]7;file://host{path}").into_bytes();
        bytes.extend_from_slice(terminator);
        for chunk in [1, 2, 3, 7, 64] {
            let mut actual = Term::new(24, 6, 100);
            for part in bytes.chunks(chunk) {
                actual.advance(part);
            }
            assert_eq!(
                actual.cwd.as_deref(),
                Some(path),
                "terminator={terminator:?}, chunk={chunk}"
            );
            let cwd_events = actual
                .take_events()
                .into_iter()
                .filter(|event| matches!(event, Event::Cwd(_)))
                .count();
            assert_eq!(cwd_events, 1, "one completed OSC produces one event");
        }
    }
}

#[test]
fn standalone_c1_osc_is_still_recognized() {
    for chunk in [1, 2, 3, 64] {
        let mut actual = Term::new(24, 6, 100);
        for part in b"\x9d7;file://host/tmp\x07".chunks(chunk) {
            actual.advance(part);
        }
        assert_eq!(actual.cwd.as_deref(), Some("/tmp"), "chunk={chunk}");
    }
}
