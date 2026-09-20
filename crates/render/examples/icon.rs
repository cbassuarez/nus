//! Write the bundled app icon: `cargo run -p nus-render --example icon [dir]`.
//! White desktop mark/red orbit; the running app follows the surface colour.

use nus_render::icon::{app_icon_svg, band_stops, ico, png};
use nus_render::theme::signal;

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "assets/icon".into());
    std::fs::create_dir_all(&dir).unwrap();
    let mut entries = Vec::new();
    for size in [16u32, 24, 32, 48, 64, 128, 256, 512, 1024] {
        let rgba = nus_render::dock_icon::render(
            size,
            signal::RED,
            nus_render::dock_icon::Face::Newsreader,
        );
        let p = png(&rgba, size, size);
        std::fs::write(format!("{dir}/nus-{size}.png"), &p).unwrap();
        if size <= 256 {
            entries.push((size, p));
        }
    }
    std::fs::write(format!("{dir}/nus.ico"), ico(&entries)).unwrap();
    // Dock variant (pure white n, clipped by its orbit, with a contrast shadow), at every
    // size a macOS .icns wants (scripts/bundle-mac.sh builds it from these).
    for size in [16u32, 32, 64, 128, 256, 512, 1024] {
        let rgba = nus_render::dock_icon::render(
            size,
            signal::RED,
            nus_render::dock_icon::Face::Newsreader,
        );
        std::fs::write(format!("{dir}/nus-{size}-ink.png"), png(&rgba, size, size)).unwrap();
    }
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
