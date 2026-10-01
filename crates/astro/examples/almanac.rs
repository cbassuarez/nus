//! `cargo run -p nus-astro --example almanac -- <lat> <lon> [unix_ms] [tz_minutes]`
//! Prints what `sky`, `moon` and `tonight` say.
use nus_astro::{moon_report, sky_report, tonight_report, Observer, Report};

fn show(r: Report) {
    println!("\n{}", r.title);
    for l in r.lines {
        println!("  {l}");
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: f64| a.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
    let o = Observer::new(num(1, 40.7), num(2, -74.0));
    let ms = num(
        3,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0),
    );
    let tz = num(4, 0.0) as i32;
    show(sky_report(ms, Some(&o), tz));
    show(moon_report(ms, Some(&o), tz));
    show(tonight_report(ms, Some(&o), tz));
}
