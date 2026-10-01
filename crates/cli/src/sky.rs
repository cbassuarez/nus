//! `nus sky`, `nus moon`, `nus tonight`: the almanac, from a shell. Computed here, on this
//! machine, from the place chosen in nus (profile/settings.json) or given with
//! `--place LAT,LON`; nothing is looked up and nothing is sent anywhere.

use std::path::PathBuf;
use std::process::ExitCode;

use nus_astro as astro;

/// The profile folder beside the running instance, where settings.json lives.
fn profile_dir() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("NUS_INSTANCE")
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(PathBuf::from)),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("profile"))),
        std::env::current_dir().ok().map(|d| d.join("profile")),
        std::env::current_dir()
            .ok()
            .map(|d| d.join("spikes").join("composite").join("profile")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|d| d.join("settings.json").is_file())
}

/// The place chosen in nus, if one is.
fn chosen_place() -> Option<(f64, f64)> {
    let text = std::fs::read_to_string(profile_dir()?.join("settings.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let p = value.get("behavior")?.get("place")?.as_array()?;
    Some((p.first()?.as_f64()?, p.get(1)?.as_f64()?))
}

fn parse_place(text: &str) -> Option<(f64, f64)> {
    let (a, b) = text.split_once(',')?;
    let (lat, lon) = (a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?);
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some((lat, lon))
}

pub fn run(which: &str, args: &[String]) -> ExitCode {
    let mut place = None;
    let mut at = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--place" => match it.next().and_then(|v| parse_place(v)) {
                Some(p) => place = Some(p),
                None => {
                    eprintln!("nus: --place wants LAT,LON, like 35.2,-106.6");
                    return ExitCode::from(2);
                }
            },
            "--at" => match it.next().and_then(|v| v.parse::<f64>().ok()) {
                Some(ms) => at = Some(ms),
                None => {
                    eprintln!("nus: --at wants a unix time in milliseconds");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("nus: {which}: unexpected {other:?}\nusage: nus {which} [--place LAT,LON] [--at UNIX_MS]");
                return ExitCode::from(2);
            }
        }
    }
    let observer = place
        .or_else(chosen_place)
        .map(|(la, lo)| astro::Observer::new(la, lo));
    let ms = at.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0)
    });
    let tz = astro::local_offset_minutes();
    let report = match which {
        "moon" => astro::moon_report(ms, observer.as_ref(), tz),
        "tonight" => astro::tonight_report(ms, observer.as_ref(), tz),
        _ => astro::sky_report(ms, observer.as_ref(), tz),
    };
    println!("{}", report.title);
    for line in &report.lines {
        println!("  {line}");
    }
    if observer.is_none() && which == "moon" {
        println!("  (pass --place LAT,LON, or choose a place in nus, for rising and setting)");
    }
    ExitCode::SUCCESS
}
