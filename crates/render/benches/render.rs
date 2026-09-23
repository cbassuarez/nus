use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use nus_render::{
    policy::{ensure_contrast, nearest},
    scene::{Instance, Rect},
    text::bundled,
    FontSystem, Scene, Style,
};
use nus_vt::Rgb;

fn fonts() -> (FontSystem, nus_render::FontId) {
    let mut fonts = FontSystem::new();
    let font = fonts
        .load_bytes(bundled::PLEX_MONO, 0)
        .expect("bundled Plex Mono");
    (fonts, font)
}

fn bench_shape(c: &mut Criterion) {
    let cases = [
        ("hot_prompt", "seb@nus ~/src % cargo test".to_string()),
        ("hot_256b", "x".repeat(256)),
        ("uncached_513b", "x".repeat(513)),
        ("uncached_4k", "x".repeat(4096)),
    ];
    let mut group = c.benchmark_group("render/shape");
    for (name, text) in cases {
        let (fonts, font) = fonts();
        let warm = fonts.shape(font, 14.0, &text);
        assert!(!warm.is_empty());
        group.bench_with_input(BenchmarkId::from_parameter(name), &text, |b, text| {
            b.iter(|| black_box(fonts.shape(font, 14.0, black_box(text))));
        });
    }
    group.finish();
}

fn bench_measure(c: &mut Criterion) {
    let (fonts, font) = fonts();
    let style = Style {
        font,
        px: 14.0,
        color: [1.0; 4],
        tracking: 0.0,
    };
    let text = "cargo test --workspace --all-targets";
    let width = fonts.measure(style, text);
    assert!(width > 0.0);
    c.bench_function("render/measure/hot_shell_line", |b| {
        b.iter(|| black_box(fonts.measure(style, black_box(text))));
    });
}

fn bench_glyph(c: &mut Criterion) {
    let mut group = c.benchmark_group("render/glyph");
    group.bench_function("hot_ascii", |b| {
        let (mut fonts, font) = fonts();
        let id = fonts.shape(font, 14.0, "M")[0].id;
        assert!(fonts.glyph(font, 14.0, id).is_some());
        b.iter(|| black_box(fonts.glyph(font, 14.0, black_box(id))));
    });
    group.bench_function("cold_ascii_v2", |b| {
        b.iter_batched_ref(
            || {
                let (fonts, font) = fonts();
                let id = fonts.shape(font, 14.0, "M")[0].id;
                (fonts, font, id)
            },
            |(fonts, font, id)| black_box(fonts.glyph(*font, 14.0, black_box(*id))),
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn push_terminal_scene(scene: &mut Scene, cols: usize, rows: usize) {
    scene.rect(
        Rect::new(0.0, 0.0, cols as f32 * 8.0, rows as f32 * 16.0),
        [0.03, 0.03, 0.03, 1.0],
    );
    for row in 0..rows {
        for col in 0..cols {
            let x = col as f32 * 8.0;
            let y = row as f32 * 16.0;
            scene.push(Instance::glyph(
                x,
                y,
                7.0,
                14.0,
                [0.0, 0.0, 0.01, 0.01],
                [0.9, 0.9, 0.88, 1.0],
            ));
        }
    }
    scene.rect(Rect::new(16.0, 16.0, 8.0, 16.0), [0.8, 0.8, 0.8, 0.7]);
}

fn bench_scene(c: &mut Criterion) {
    let mut group = c.benchmark_group("render/scene");
    for (cols, rows) in [(80usize, 24usize), (120, 40), (200, 60)] {
        let name = format!("terminal_{cols}x{rows}_v2");
        group.bench_function(name, |b| {
            b.iter_batched_ref(
                Scene::new,
                |scene| {
                    push_terminal_scene(scene, cols, rows);
                    scene.finish();
                    black_box(scene.instances());
                    black_box(scene.layers());
                },
                BatchSize::PerIteration,
            );
        });
    }
    for count in [1_000usize, 10_000] {
        group.bench_with_input(BenchmarkId::new("finish_v2", count), &count, |b, &count| {
            b.iter_batched_ref(
                || {
                    let mut scene = Scene::new();
                    for i in 0..count {
                        scene.push(Instance::rect(Rect::new(i as f32, 0.0, 1.0, 1.0), [1.0; 4]));
                    }
                    scene
                },
                |scene| {
                    scene.finish();
                    black_box(scene.layers());
                },
                // finish() only closes a layer. Exclude freeing every instance;
                // cap the batch to 16 scenes (about 10 MiB at 10k instances).
                BatchSize::NumIterations(16),
            );
        });
    }
    group.finish();
}

fn bench_policy(c: &mut Criterion) {
    let palette = [
        Rgb {
            r: 20,
            g: 20,
            b: 20,
        },
        Rgb {
            r: 224,
            g: 87,
            b: 76,
        },
        Rgb {
            r: 122,
            g: 199,
            b: 127,
        },
        Rgb {
            r: 229,
            g: 185,
            b: 74,
        },
        Rgb {
            r: 127,
            g: 167,
            b: 240,
        },
        Rgb {
            r: 216,
            g: 143,
            b: 216,
        },
        Rgb {
            r: 111,
            g: 211,
            b: 220,
        },
        Rgb {
            r: 216,
            g: 210,
            b: 196,
        },
        Rgb {
            r: 107,
            g: 102,
            b: 92,
        },
        Rgb {
            r: 255,
            g: 122,
            b: 110,
        },
        Rgb {
            r: 147,
            g: 226,
            b: 154,
        },
        Rgb {
            r: 255,
            g: 208,
            b: 106,
        },
        Rgb {
            r: 143,
            g: 188,
            b: 255,
        },
        Rgb {
            r: 233,
            g: 160,
            b: 233,
        },
        Rgb {
            r: 139,
            g: 230,
            b: 239,
        },
        Rgb {
            r: 236,
            g: 231,
            b: 218,
        },
    ];
    c.bench_function("render/policy/ensure_contrast", |b| {
        b.iter(|| {
            black_box(ensure_contrast(
                black_box([0.3, 0.3, 0.3, 1.0]),
                [0.08, 0.08, 0.08, 1.0],
                4.5,
            ))
        });
    });
    c.bench_function("render/policy/nearest_palette", |b| {
        b.iter(|| {
            black_box(nearest(
                black_box(Rgb {
                    r: 202,
                    g: 111,
                    b: 44,
                }),
                &palette,
            ))
        });
    });
}

criterion_group!(
    benches,
    bench_shape,
    bench_measure,
    bench_glyph,
    bench_scene,
    bench_policy
);
criterion_main!(benches);
