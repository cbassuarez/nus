use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use nus_sync::{decode_key, encode_key, open, seal, union_lines, KEY_LEN};

const KEY: [u8; KEY_LEN] = [0x42; KEY_LEN];
const PATH: &str = "profile/memory.md";

fn line_file(prefix: &str, count: usize, start: usize) -> String {
    let mut out = String::with_capacity(count * 32);
    for i in start..start + count {
        out.push_str(prefix);
        out.push(' ');
        out.push_str(&format!("{i:08}"));
        out.push('\n');
    }
    out
}

fn bench_union(c: &mut Criterion) {
    let identical = line_file("memory", 10_000, 0);
    let disjoint_a = line_file("theirs", 10_000, 0);
    let disjoint_b = line_file("ours", 10_000, 10_000);
    let overlap_a = line_file("memory", 10_000, 0);
    let overlap_b = line_file("memory", 10_000, 5_000);
    let cases = [
        ("identical_10k", identical.as_str(), identical.as_str()),
        ("disjoint_10k", disjoint_a.as_str(), disjoint_b.as_str()),
        ("half_overlap_10k", overlap_a.as_str(), overlap_b.as_str()),
    ];
    let mut group = c.benchmark_group("sync/union_lines");
    for (name, theirs, ours) in cases {
        group.throughput(Throughput::Bytes((theirs.len() + ours.len()) as u64));
        group.bench_function(name, |b| {
            b.iter(|| black_box(union_lines(black_box(theirs), black_box(ours))))
        });
    }
    group.finish();
}

fn bench_key(c: &mut Criterion) {
    let encoded = encode_key(&KEY);
    assert_eq!(decode_key(&encoded), Some(KEY));
    c.bench_function("sync/key/encode", |b| {
        b.iter(|| black_box(encode_key(black_box(&KEY))))
    });
    c.bench_function("sync/key/decode", |b| {
        b.iter(|| black_box(decode_key(black_box(&encoded))))
    });
}

fn bench_open(c: &mut Criterion) {
    // seal() intentionally stays outside the measured region: it obtains a
    // fresh OS nonce and would make a CodSpeed simulation benchmark depend on
    // system randomness rather than only the deterministic decrypt path.
    let mut group = c.benchmark_group("sync/open");
    for size in [1024usize, 64 * 1024, 1024 * 1024] {
        let plain: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let sealed = seal(&KEY, PATH, &plain);
        assert_eq!(open(&KEY, PATH, &sealed).as_deref(), Some(plain.as_slice()));
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &sealed, |b, sealed| {
            b.iter(|| black_box(open(black_box(&KEY), PATH, black_box(sealed))));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_union, bench_key, bench_open);
criterion_main!(benches);
