//! Export the exact desktop animation bank for artifact review, without a UI.
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let art = image::open(&args[1]).expect("256px approved Mercury PNG").to_rgba8();
    assert_eq!(art.dimensions(), (256, 256));
    let output = std::path::Path::new(&args[2]);
    std::fs::create_dir_all(output).unwrap();
    let start = std::time::Instant::now();
    let frames = nus_render::mercury::frames(256, art.as_raw());
    for (kind, bank) in [("intro", &frames.intro), ("idle", &frames.idle)] {
        let dir = output.join(kind); std::fs::create_dir_all(&dir).unwrap();
        for (i, png) in bank.iter().enumerate() { std::fs::write(dir.join(format!("{i:03}.png")), png).unwrap(); }
    }
    println!("{} intro + {} idle frames in {:?}", frames.intro.len(), frames.idle.len(), start.elapsed());
}
