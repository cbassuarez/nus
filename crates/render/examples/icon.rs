//! Write the bundled app icon: `cargo run -p nus-render --example icon [dir]`.
//! Default paper/red; the running app regenerates it from the surface.

use nus_render::icon::{app_icon, app_icon_svg, band_stops, ico, png};
use nus_render::theme::{signal, Theme};

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "assets/icon".into());
    std::fs::create_dir_all(&dir).unwrap();
    let ink = Theme::paper().ink;
    let mut entries = Vec::new();
    for size in [16u32, 24, 32, 48, 64, 128, 256, 512, 1024] {
        let rgba = app_icon(size, ink, signal::RED);
        let p = png(&rgba, size, size);
        std::fs::write(format!("{dir}/nus-{size}.png"), &p).unwrap();
        if size <= 256 {
            entries.push((size, p));
        }
    }
    std::fs::write(format!("{dir}/nus.ico"), ico(&entries)).unwrap();
    // Ink variant for dark docks/taskbars.
    let rgba = app_icon(512, Theme::ink().ink, signal::RED);
    std::fs::write(format!("{dir}/nus-512-ink.png"), png(&rgba, 512, 512)).unwrap();
    // The vector, for the site and anything that scales.
    std::fs::write(
        format!("{dir}/nus.svg"),
        app_icon_svg(512.0, "#141413", "#c8102e"),
    )
    .unwrap();
    // Where four stops sit clear of the n, for anything laying out the plate.
    let stops: Vec<String> = band_stops(512.0, 4)
        .iter()
        .map(|t| format!("{t:.3}"))
        .collect();
    std::fs::write(
        format!("{dir}/stops.json"),
        format!("[{}]", stops.join(",")),
    )
    .unwrap();
    println!("wrote {dir}");
}
