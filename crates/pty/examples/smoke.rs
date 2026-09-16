//! Headless smoke test: spawn the default shell, run a command, print what
//! the terminal grid contains after a few seconds.

use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let (cols, rows) = (67u16, 34u16);
    let profile = nus_pty::Profile::default_shell();
    eprintln!("profile: {} {:?}", profile.program, profile.args);
    let mut pty = nus_pty::Pty::spawn(&profile, cols, rows, || {})?;
    let mut term = nus_vt::Term::new(cols as usize, rows as usize, 1000);

    let start = Instant::now();
    let mut sent = false;
    while start.elapsed() < Duration::from_secs(4) {
        let out = pty.take_output();
        if !out.is_empty() {
            term.advance(&out);
            let responses = term.take_responses();
            pty.write(&responses)?;
        }
        if !sent && start.elapsed() > Duration::from_millis(1500) {
            eprintln!(
                "before input: cursor {:?}, wrap_next {}",
                (term.cursor().row, term.cursor().col),
                term.cursor().wrap_next
            );
            for r in 0..6 {
                eprintln!("  row {r}: {:?}", term.grid().row(r).text());
            }
            pty.write(b"echo nus-ok-$([int]([math]::Pow(6,2)))\r")?;
            sent = true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    println!(
        "--- grid ({}x{}), cursor {:?} ---",
        term.cols(),
        term.rows(),
        (term.cursor().row, term.cursor().col)
    );
    println!("{}", term.grid().text());
    println!("--- title: {:?}, modes: {:?}", term.title(), term.modes());
    Ok(())
}
