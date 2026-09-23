mod support;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use support::{fixture, populated_term, scroll_full_screen, validate, TerminalFixture};

// Large grids must not be batched by the thousands. PerIteration bounds live
// state to one terminal; iter_batched_ref excludes both setup and its destructor.
// Changed timing boundaries get new IDs: v1 data is NOT a speedup baseline.
fn bench_advance(c: &mut Criterion) {
    let cases = [
        (
            "plain_ascii_64k",
            TerminalFixture::PlainAscii { bytes: 64 * 1024 },
        ),
        ("ansi_log_2k", TerminalFixture::AnsiLog { lines: 2_000 }),
        (
            "cursor_heavy_2k",
            TerminalFixture::CursorHeavy { frames: 2_000 },
        ),
        (
            "tui_120x40x20",
            TerminalFixture::TuiRedraw {
                rows: 40,
                cols: 120,
                frames: 20,
            },
        ),
        (
            "combining_unicode",
            TerminalFixture::CombiningUnicode { graphemes: 16_000 },
        ),
        (
            "wide_unicode",
            TerminalFixture::WideUnicode { cells: 16_000 },
        ),
        (
            "scroll_10k",
            TerminalFixture::ScrollStream { lines: 10_000 },
        ),
    ];
    let deliveries: [(&str, usize); 3] = [
        ("whole", usize::MAX),
        ("chunk_64", 64),
        ("chunk_4096", 4096),
    ];
    let mut group = c.benchmark_group("vt/advance_v2");
    for (name, case) in cases {
        let bytes = fixture(case);
        validate(case, &bytes);
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        for (delivery, chunk) in deliveries {
            group.bench_with_input(BenchmarkId::new(name, delivery), &bytes, |b, input| {
                b.iter_batched_ref(
                    || nus_vt::Term::new(120, 40, 10_000),
                    |term| {
                        if chunk == usize::MAX {
                            term.advance(black_box(input));
                        } else {
                            for piece in input.chunks(chunk) {
                                term.advance(black_box(piece));
                            }
                        }
                        black_box(&*term);
                    },
                    BatchSize::PerIteration,
                );
            });
        }
    }
    group.finish();
}

fn bench_search(c: &mut Criterion) {
    let term = populated_term(120, 40, 10_000, 10_000);
    let cases = [
        ("match_early", "00000010"),
        ("match_late", "00009990"),
        ("no_match", "definitely-not-in-the-buffer"),
        ("many_matches", "cargo"),
    ];
    let mut group = c.benchmark_group("vt/search");
    for (name, query) in cases {
        group.bench_function(name, |b| {
            b.iter(|| black_box(term.search(black_box(query))))
        });
    }
    group.finish();
}

fn bench_resize(c: &mut Criterion) {
    let mut group = c.benchmark_group("vt/resize_v2");
    for (name, from, to) in [
        ("80x24_to_120x40", (80, 24), (120, 40)),
        ("120x40_to_80x24", (120, 40), (80, 24)),
        ("120x40_to_200x60", (120, 40), (200, 60)),
    ] {
        group.bench_function(name, |b| {
            b.iter_batched_ref(
                || populated_term(from.0, from.1, 10_000, 5_000),
                |term| {
                    term.resize(to.0, to.1);
                    black_box(&*term);
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

fn bench_grid_scroll(c: &mut Criterion) {
    let base = populated_term(120, 40, 10_000, 4_000).grid().clone();
    let mut group = c.benchmark_group("vt/scroll_v2");
    for n in [1usize, 10, 40] {
        // Validate the very same operation before Criterion starts sampling.
        let mut check = base.clone();
        scroll_full_screen(&mut check, n);
        assert_eq!(check.rows(), base.rows());
        assert_eq!(check.scrollback_len(), base.scrollback_len() + n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched_ref(
                || base.clone(),
                |grid| {
                    scroll_full_screen(grid, n);
                    black_box(&*grid);
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_advance,
    bench_search,
    bench_resize,
    bench_grid_scroll
);
criterion_main!(benches);
