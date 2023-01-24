use std::time::{Duration, Instant};

use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use grex_t0::{
    capture::PAYLOAD_SIZE,
    common::{Payload, CHANNELS},
    dumps::DumpRing,
};
use rand::prelude::*;

fn decode(c: &mut Criterion) {
    let mut rng = rand::thread_rng();
    c.bench_function("payload unpack", |b| {
        b.iter_batched(
            || {
                // Setup by creating random bytes
                let mut bytes = [0u8; PAYLOAD_SIZE];
                rng.fill(&mut bytes[..]);
                bytes
            },
            |bytes| {
                // Execute
                Payload::from_bytes(black_box(&bytes))
            },
            BatchSize::SmallInput,
        )
    });
}

fn downsample_stokes(c: &mut Criterion) {
    let mut rng = rand::thread_rng();
    let mut group = c.benchmark_group("downsample_stokes");
    for downsample_factor in [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(downsample_factor),
            downsample_factor,
            |b, &downsample_factor| {
                b.iter_custom(|iters| {
                    // Create payloads
                    let mut payloads = vec![];
                    for _i in 0..iters {
                        let mut bytes = [0u8; PAYLOAD_SIZE];
                        rng.fill(&mut bytes[..]);
                        payloads.push(Payload::from_bytes(&bytes));
                    }

                    // Setup state
                    let mut avg = [0f32; CHANNELS];
                    let mut idx = 0usize;

                    let start = Instant::now();
                    for i in 0..iters {
                        avg.iter_mut()
                            .zip(payloads[i as usize].stokes_i())
                            .for_each(|(x, y)| *x += f32::from(y));
                        // If we're at the end, calculate the average
                        if idx == downsample_factor - 1 {
                            // Find the average into an f32 (which is lossless)
                            avg.iter_mut()
                                .for_each(|v| *v /= f32::from(downsample_factor as u16));
                            // And zero the state
                            avg = [0f32; CHANNELS];
                        }
                        // Increment the idx
                        idx = (idx + 1) % downsample_factor;
                    }
                    start.elapsed()
                })
            },
        );
    }
    group.finish();
}

fn pack_ring(c: &mut Criterion) {
    let mut group = c.benchmark_group("pack ring");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(30));
    group.bench_function("pack ring", |b| {
        b.iter_batched(
            // I can't benchmark anymore on my home PC because I don't have enough RAM
            || DumpRing::new(1_048_576),
            |dr| black_box(dr.pack()),
            BatchSize::PerIteration,
        )
    });
    group.finish();
}

pub fn push_ring(c: &mut Criterion) {
    let mut dr = DumpRing::new(1_048_576);
    let pl = Payload::default();
    c.bench_function("push ring", |b| {
        b.iter(|| {
            dr.push(black_box(pl));
        })
    });
}

criterion_group!(benches, push_ring, downsample_stokes, pack_ring, decode);
criterion_main!(benches);
