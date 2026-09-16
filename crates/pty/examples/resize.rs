//! What does ConPTY send when the PTY is resized? Dump the raw bytes.

use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let profile = nus_pty::Profile::default_shell();
    let mut pty = nus_pty::Pty::spawn(&profile, 67, 34, || {})?;
    let mut term = nus_vt::Term::new(67, 34, 1000);
    let mut phase = 0;
    let start = Instant::now();
    let mut raw = Vec::new();
    while start.elapsed() < Duration::from_secs(5) {
        let out = pty.take_output();
        if !out.is_empty() {
            if phase >= 1 {
                raw.extend_from_slice(&out);
            }
            term.advance(&out);
            pty.write(&term.take_responses())?;
        }
        if phase == 0 && start.elapsed() > Duration::from_millis(1500) {
            eprintln!(
                "before resize: cursor {:?}",
                (term.cursor().row, term.cursor().col)
            );
            pty.resize(67, 31, (13, 25))?;
            term.resize(67, 31);
            eprintln!(
                "after Term::resize: cursor {:?}",
                (term.cursor().row, term.cursor().col)
            );
            phase = 1;
        }
        if phase == 1 && start.elapsed() > Duration::from_millis(3000) {
            eprintln!("ConPTY sent after resize ({} bytes):", raw.len());
            eprintln!("{}", String::from_utf8_lossy(&raw).replace('', "<ESC>"));
            eprintln!("cursor now {:?}", (term.cursor().row, term.cursor().col));
            for r in 0..4 {
                eprintln!("  row {r}: {:?}", term.grid().row(r).text());
            }
            phase = 2;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
