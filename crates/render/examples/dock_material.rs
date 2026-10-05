//! Export the runtime material renderer for comparison against Blender proofs.
use nus_render::{
    dock_icon::{self, Face},
    icon::png,
    theme::hex,
};
fn main() {
    let dir = std::env::args().nth(1).expect("output directory");
    std::fs::create_dir_all(&dir).unwrap();
    let names = [
        "plex",
        "silkscreen",
        "plex-italic",
        "bungee",
        "rubik",
        "newsreader",
    ];
    let start = std::time::Instant::now();
    for (face, name) in Face::ALL.into_iter().zip(names) {
        for size in [16, 24, 32, 48, 64, 128, 256, 512, 1024] {
            let pixels = dock_icon::render(size, hex(0xc8102e), face);
            std::fs::write(format!("{dir}/{name}-{size}.png"), png(&pixels, size, size)).unwrap();
        }
    }
    println!("six banks and size proofs: {:?}", start.elapsed());
    for rgb in [
        0xc8102e, 0x88c0d0, 0xbd93f9, 0xe3b341, 0x071129, 0xffffff, 0x49ba9c,
    ] {
        let pixels = dock_icon::render(1024, hex(rgb), Face::Newsreader);
        std::fs::write(
            format!("{dir}/signal-{rgb:06x}.png"),
            png(&pixels, 1024, 1024),
        )
        .unwrap();
        let pixels = dock_icon::render(256, hex(rgb), Face::Newsreader);
        std::fs::write(
            format!("{dir}/signal-{rgb:06x}-256.png"),
            png(&pixels, 256, 256),
        )
        .unwrap();
    }
    let start = std::time::Instant::now();
    for face in Face::ALL {
        std::hint::black_box(dock_icon::render(256, hex(0x49ba9c), face));
    }
    println!("warm six-face signal update: {:?}", start.elapsed());
}
