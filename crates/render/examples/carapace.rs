//! GPU material acceptance checks and a gallery rendered by the actual quad
//! shader: `cargo run -p nus-render --example carapace -- /tmp/nus-carapace`.
//! Gallery rows: ink pool, enamel, interference, single seam, open corners,
//! overprint, edge light. Columns: paper at rest; dark with expressive activity.

use std::{path::PathBuf, sync::Arc};

use nus_render::{CarapaceLook, Gpu, Instance, Rect, Scene};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

const SIZE: (u32, u32) = (320, 180);
const FRAME: Rect = Rect::new(8.0, 8.0, 304.0, 164.0);

fn pixels(gpu: &mut Gpu, look: CarapaceLook, clip: Option<Rect>) -> Vec<u8> {
    let mut scene = Scene::new();
    scene.layer(clip);
    scene.push(Instance::carapace(FRAME, look));
    scene.finish();
    gpu.snapshot_alpha(SIZE, &scene, [0.0; 4])
}

fn alpha(pixels: &[u8], x: u32, y: u32) -> u8 {
    pixels[((y * SIZE.0 + x) * 4 + 3) as usize]
}

fn verify(gpu: &mut Gpu) {
    for material in 1..=7 {
        for width in [1.0, 6.0, 12.0] {
            for radius in [0.0, 1.0, 16.0] {
                let look = CarapaceLook {
                    material,
                    width,
                    radius,
                    ..Default::default()
                };
                let rest = pixels(gpu, look, None);
                let mut still_other_phase = Instance::carapace(FRAME, look);
                // Bypass the constructor's still guard to verify the shader's
                // own zero-energy contract as well.
                still_other_phase.phase = 0.73;
                let mut scene = Scene::new();
                scene.push(still_other_phase);
                scene.finish();
                let other = gpu.snapshot_alpha(SIZE, &scene, [0.0; 4]);
                assert_eq!(
                    rest, other,
                    "still changed with phase: material {material}, width {width}, radius {radius}"
                );
                assert_eq!(
                    alpha(&rest, 160, 90),
                    0,
                    "material {material} reached the content"
                );
                assert_eq!(
                    alpha(&rest, 0, 0),
                    0,
                    "material {material} escaped its rect"
                );
                assert!(
                    rest.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
                    "material {material} disappeared at width {width}"
                );
                let grain = pixels(gpu, CarapaceLook { grain: 0.3, ..look }, None);
                assert!(
                    rest.as_chunks::<4>().0.iter()
                        .zip(grain.as_chunks::<4>().0.iter())
                        .all(|(a, b)| a[3] == b[3]),
                    "grain altered the silhouette: material {material}, width {width}, radius {radius}"
                );
            }
        }
        let look = CarapaceLook {
            material,
            width: 12.0,
            radius: 16.0,
            ..Default::default()
        };
        let a = pixels(
            gpu,
            CarapaceLook {
                phase: 0.15,
                energy: 1.0,
                ..look
            },
            None,
        );
        let b = pixels(
            gpu,
            CarapaceLook {
                phase: 0.60,
                energy: 1.0,
                ..look
            },
            None,
        );
        assert_ne!(a, b, "material {material} did not respond to activity");
        let progress_a = pixels(
            gpu,
            CarapaceLook {
                progress: Some(0.4),
                phase: 0.15,
                energy: 1.0,
                ..look
            },
            None,
        );
        let progress_b = pixels(
            gpu,
            CarapaceLook {
                progress: Some(0.4),
                phase: 0.60,
                energy: 1.0,
                ..look
            },
            None,
        );
        assert_eq!(
            progress_a, progress_b,
            "determinate progress followed the clock: material {material}"
        );
        let progress_c = pixels(
            gpu,
            CarapaceLook {
                progress: Some(0.8),
                phase: 0.15,
                energy: 1.0,
                ..look
            },
            None,
        );
        assert_ne!(
            progress_a, progress_c,
            "material {material} did not reflect real progress"
        );
        let band = pixels(
            gpu,
            CarapaceLook {
                band: true,
                grain: 0.3,
                ..look
            },
            None,
        );
        assert_eq!(
            alpha(&band, 12, 90),
            0,
            "material {material} band painted a side"
        );
        assert_eq!(
            alpha(&band, 160, 168),
            0,
            "material {material} band painted its bottom"
        );
        let clipped = pixels(gpu, look, Some(Rect::new(0.0, 0.0, 160.0, 180.0)));
        assert!(
            (0..SIZE.1).all(|y| (160..SIZE.0).all(|x| alpha(&clipped, x, y) == 0)),
            "material {material} escaped its layer clip"
        );
        if material == 5 {
            let corners = pixels(gpu, CarapaceLook { grain: 0.3, ..look }, None);
            assert_eq!(alpha(&corners, 160, 13), 0, "open corner span was filled");
            assert_eq!(
                alpha(&corners, 13, 90),
                0,
                "open corner side span was filled"
            );
        }
        if material == 4 {
            for y in 0..SIZE.1 {
                for x in 0..SIZE.0 {
                    if !(214..=227).contains(&x) || y >= 20 {
                        let i = ((y * SIZE.0 + x) * 4) as usize;
                        assert_eq!(
                            &a[i..i + 4],
                            &b[i..i + 4],
                            "seam activity moved outside its joint"
                        );
                    }
                }
            }
        }
    }
    println!(
        "GPU checks passed: seven materials; widths 1/6/12; radii 0/1/16; still/activity; determinate progress independent of clock; content bounds; grain alpha; top bands; open spans; joint locality; layer clips."
    );
}

fn gallery(gpu: &mut Gpu, dir: &std::path::Path) {
    let mut scene = Scene::new();
    let size = (1200, 1190);
    scene.rect(
        Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32),
        [0.83, 0.83, 0.80, 1.0],
    );
    for material in 1..=7 {
        for column in 0..2 {
            let dark = column == 1;
            let paper = if dark {
                [0.075, 0.086, 0.098, 1.0]
            } else {
                [0.95, 0.94, 0.91, 1.0]
            };
            let frame = Rect::new(
                20.0 + column as f32 * 594.0,
                16.0 + (material - 1) as f32 * 168.0,
                570.0,
                150.0,
            );
            let width = if material == 1 { 12.0 } else { 8.0 };
            scene.push(Instance::rounded(frame, 16.0, paper));
            scene.push(Instance::carapace(
                frame,
                CarapaceLook {
                    material,
                    radius: 16.0,
                    width,
                    paper,
                    signal: [0.21, 0.40, 0.46, 1.0],
                    energy: if dark { 1.0 } else { 0.0 },
                    phase: 0.21,
                    grain: 0.045,
                    ..Default::default()
                },
            ));
            let inner = frame.inset(width + 1.0);
            let rule = if dark {
                [0.20, 0.23, 0.25, 1.0]
            } else {
                [0.76, 0.76, 0.72, 1.0]
            };
            scene.rect(
                Rect::new(inner.x + 14.0, inner.y + 22.0, inner.w * 0.16, 3.0),
                rule,
            );
            for row in 0..3 {
                scene.rect(
                    Rect::new(
                        inner.x + 14.0,
                        inner.y + 52.0 + row as f32 * 14.0,
                        inner.w * (0.63 - row as f32 * 0.11),
                        1.0,
                    ),
                    rule,
                );
            }
        }
    }
    scene.finish();
    let rgba = gpu.snapshot(size, &scene, [0.0; 4]);
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("carapace-gallery.png");
    std::fs::write(&path, nus_render::icon::png(&rgba, size.0, size.1)).unwrap();
    println!("Gallery: {}", path.display());
}

struct Harness(PathBuf);

impl ApplicationHandler for Harness {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .unwrap(),
        );
        let (mut gpu, _target) = Gpu::new(window).unwrap();
        verify(&mut gpu);
        gallery(&mut gpu, &self.0);
        event_loop.exit();
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

fn main() {
    let dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/nus-carapace"));
    EventLoop::new()
        .unwrap()
        .run_app(&mut Harness(dir))
        .unwrap();
}
