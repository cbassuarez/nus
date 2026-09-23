use std::{hint::black_box, io::Cursor};

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use nus_pty::hold::{parse_frames, recv, send, Ring};

fn encoded(payload_size: usize, frames: usize) -> Vec<u8> {
    let payload: Vec<u8> = (0..payload_size).map(|i| (i % 251) as u8).collect();
    let mut out = Vec::with_capacity(frames * (payload_size + 5));
    for _ in 0..frames {
        send(&mut out, b'o', &payload).expect("encode frame");
    }
    out
}

fn bench_frame_codec(c: &mut Criterion) {
    let mut encode = c.benchmark_group("pty/frame_encode");
    for size in [64usize, 4 * 1024, 64 * 1024] {
        let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        encode.throughput(Throughput::Bytes(size as u64));
        encode.bench_with_input(BenchmarkId::from_parameter(size), &payload, |b, payload| {
            b.iter(|| {
                let mut out = Vec::with_capacity(payload.len() + 5);
                send(&mut out, b'o', black_box(payload)).expect("encode frame");
                black_box(out)
            });
        });
    }
    encode.finish();

    let mut decode = c.benchmark_group("pty/frame_decode");
    for size in [64usize, 4 * 1024, 64 * 1024] {
        let bytes = encoded(size, 1);
        decode.throughput(Throughput::Bytes(size as u64));
        decode.bench_with_input(BenchmarkId::from_parameter(size), &bytes, |b, bytes| {
            b.iter(|| {
                let mut cursor = Cursor::new(black_box(bytes.as_slice()));
                black_box(recv(&mut cursor).expect("decode frame"))
            });
        });
    }
    decode.finish();
}

fn bench_parse_frames(c: &mut Criterion) {
    let bytes = encoded(256, 256);
    let payload_bytes = 256u64 * 256;
    let expected_payload: Vec<u8> = (0..256).map(|i| (i % 251) as u8).collect();
    let mut group = c.benchmark_group("pty/frame_parse_v2");
    group.throughput(Throughput::Bytes(payload_bytes));
    {
        let mut sanity = bytes.clone();
        assert_eq!(parse_frames(&mut sanity).len(), 256);
        assert!(sanity.is_empty());
    }
    group.bench_function("whole", |b| {
        b.iter_batched_ref(
            || bytes.clone(),
            |pending| {
                // Observe payloads before freeing them. Both delivery modes
                // measure parsing + frame disposal, but NOT input teardown.
                let frames = black_box(parse_frames(pending));
                black_box(frames.len());
            },
            BatchSize::NumIterations(16),
        );
    });
    for chunk in [1usize, 64, 4096] {
        let mut check = Vec::new();
        let mut decoded = Vec::new();
        for part in bytes.chunks(chunk) {
            check.extend_from_slice(part);
            decoded.extend(parse_frames(&mut check));
        }
        assert!(check.is_empty());
        assert_eq!(decoded.len(), 256);
        for (tag, payload) in &decoded {
            assert_eq!(*tag, b'o');
            assert_eq!(payload.as_slice(), expected_payload.as_slice());
        }
        group.bench_with_input(
            BenchmarkId::new("fragmented", chunk),
            &chunk,
            |b, &chunk| {
                b.iter_batched_ref(
                    || Vec::with_capacity(bytes.len()),
                    |pending| {
                        let mut frames = 0usize;
                        for part in bytes.chunks(chunk) {
                            pending.extend_from_slice(black_box(part));
                            let decoded = black_box(parse_frames(pending));
                            frames += decoded.len();
                        }
                        black_box((frames, pending.len()));
                    },
                    BatchSize::NumIterations(16),
                );
            },
        );
    }
    group.finish();
}

fn bench_ring(c: &mut Criterion) {
    const CAP: usize = 4 * 1024 * 1024;
    let payload = vec![0x5a; 64 * 1024];
    let full = vec![0x11; CAP];
    let mut group = c.benchmark_group("pty/ring_v2");
    group.throughput(Throughput::Bytes(payload.len() as u64));
    group.bench_function("fill_without_wrap", |b| {
        b.iter_batched_ref(
            || Ring::new(CAP),
            |ring| {
                ring.push(black_box(&payload));
                black_box(ring.bytes());
            },
            BatchSize::PerIteration,
        );
    });
    group.bench_function("steady_state_wrap", |b| {
        b.iter_batched_ref(
            || {
                let mut ring = Ring::new(CAP);
                ring.push(&full);
                ring
            },
            |ring| {
                ring.push(black_box(&payload));
                black_box(ring.bytes());
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

criterion_group!(benches, bench_frame_codec, bench_parse_frames, bench_ring);
criterion_main!(benches);
